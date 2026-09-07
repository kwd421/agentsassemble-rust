use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, UserProfile};
use chrono::Utc;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    bootstrap::require_complete_bootstrap_in_transaction,
    human_session_authority::revalidate_human_session, profile_store::load_profile_for_identity,
};

#[derive(Clone)]
pub enum AccountAuthority {
    LocalOperator,
    BrowserDevice([u8; 32]),
    HumanSession {
        authorization: HumanSessionAuthorization,
        browser_fingerprint: [u8; 32],
    },
}

impl AccountAuthority {
    pub(crate) const fn browser_fingerprint(&self) -> Option<&[u8; 32]> {
        match self {
            Self::LocalOperator => None,
            Self::BrowserDevice(fingerprint)
            | Self::HumanSession {
                browser_fingerprint: fingerprint,
                ..
            } => Some(fingerprint),
        }
    }
}

#[derive(Clone)]
pub struct AccountIdentity {
    pub(crate) authority: AccountAuthority,
    pub(crate) user: Option<AccountUser>,
}

#[derive(Clone, serde::Serialize)]
pub struct AccountUser {
    pub user_id: String,
    pub participant_id: String,
    pub profile: UserProfile,
}

impl AccountIdentity {
    #[must_use]
    pub fn user(&self) -> Option<&AccountUser> {
        self.user.as_ref()
    }

    #[must_use]
    pub fn same_login_subject(&self, other: &Self) -> bool {
        self.user.as_ref().map(|user| &user.user_id)
            == other.user.as_ref().map(|user| &user.user_id)
            && self.authority.browser_fingerprint() == other.authority.browser_fingerprint()
    }
}

impl SqliteStore {
    /// Resolves only the account boundary's presented native, session or device authority.
    ///
    /// # Errors
    /// Rejects incomplete bootstrap, changed session/device bindings and corrupt profiles.
    pub async fn account_identity(
        &self,
        authority: AccountAuthority,
    ) -> Result<AccountIdentity, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let user = resolve_account_user(&mut transaction, &authority).await?;
        transaction.commit().await?;
        Ok(AccountIdentity { authority, user })
    }
}

pub(crate) async fn revalidate_account_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: &AccountIdentity,
) -> Result<Option<AccountUser>, PersistenceError> {
    let current = resolve_account_user(transaction, &identity.authority).await?;
    if current
        .as_ref()
        .map(|user| (&user.user_id, &user.participant_id))
        != identity
            .user
            .as_ref()
            .map(|user| (&user.user_id, &user.participant_id))
    {
        return Err(rejected(
            "account_identity_changed",
            "Account identity changed during login.",
        ));
    }
    Ok(current)
}

async fn resolve_account_user(
    transaction: &mut Transaction<'_, Sqlite>,
    authority: &AccountAuthority,
) -> Result<Option<AccountUser>, PersistenceError> {
    require_complete_bootstrap_in_transaction(transaction).await?;
    match authority {
        AccountAuthority::LocalOperator => Ok(Some(AccountUser {
            user_id: LOCAL_OPERATOR_USER_ID.into(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
            profile: load_profile_for_identity(
                transaction,
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?,
        })),
        AccountAuthority::BrowserDevice(fingerprint) => {
            let user_id = device_user_id(transaction, fingerprint).await?;
            match user_id {
                Some(user_id) => load_account_user(transaction, &user_id).await.map(Some),
                None => Ok(None),
            }
        }
        AccountAuthority::HumanSession {
            authorization,
            browser_fingerprint,
        } => {
            let (_, profile) =
                revalidate_human_session(transaction, authorization, Utc::now()).await?;
            let expected = sqlx::query_scalar::<_, Vec<u8>>("SELECT browser_credential_fingerprint FROM human_room_sessions WHERE session_fingerprint = ?")
                .bind(authorization.session_fingerprint().as_slice()).fetch_one(&mut **transaction).await?;
            if expected != browser_fingerprint.as_slice() {
                return Err(rejected(
                    "account_device_mismatch",
                    "The browser credential does not own this session.",
                ));
            }
            let principal = authorization.principal();
            if device_user_id(transaction, browser_fingerprint)
                .await?
                .is_some_and(|owner| owner != principal.principal_id)
            {
                return Err(rejected(
                    "account_device_mismatch",
                    "The browser credential belongs to another identity.",
                ));
            }
            Ok(Some(AccountUser {
                user_id: principal.principal_id.clone(),
                participant_id: principal.participant_id.clone(),
                profile,
            }))
        }
    }
}

pub(crate) async fn device_user_id(
    transaction: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
) -> Result<Option<String>, PersistenceError> {
    Ok(sqlx::query_scalar(
        "SELECT user_id FROM human_device_credentials WHERE credential_fingerprint = ?",
    )
    .bind(fingerprint.as_slice())
    .fetch_optional(&mut **transaction)
    .await?)
}

pub(crate) async fn load_account_user(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
) -> Result<AccountUser, PersistenceError> {
    let row = sqlx::query("SELECT participant_id FROM user_profiles WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| {
            rejected(
                "account_identity_missing",
                "Account profile is unavailable.",
            )
        })?;
    let participant_id: String = row.try_get("participant_id")?;
    let profile = load_profile_for_identity(transaction, user_id, &participant_id).await?;
    Ok(AccountUser {
        user_id: user_id.into(),
        participant_id,
        profile,
    })
}

pub(crate) fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
