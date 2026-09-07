use agentsassemble_domain::{RoomEvent, UserProfile};
use chrono::Utc;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    AccountIdentity, AccountUser, PersistenceError, SqliteStore,
    account_identity::{device_user_id, load_account_user, rejected, revalidate_account_identity},
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GoogleAccount {
    pub account_id: String,
}

pub struct GoogleAccountLink {
    pub account: GoogleAccount,
    pub user: AccountUser,
    pub identity_switched: bool,
    pub events: Vec<RoomEvent>,
    pub revoked_session_fingerprints: Vec<[u8; 32]>,
}

impl SqliteStore {
    /// Reads the current account only within its revalidated server identity.
    ///
    /// # Errors
    /// Rejects changed authority, corrupt links or unavailable persistence.
    pub async fn google_account(
        &self,
        identity: &AccountIdentity,
    ) -> Result<Option<GoogleAccount>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let user = revalidate_account_identity(&mut transaction, identity).await?;
        let account = match user {
            Some(user) => account_fingerprint(&mut transaction, &user.user_id)
                .await?
                .map(|fingerprint| account_view(&fingerprint)),
            None => None,
        };
        transaction.commit().await?;
        Ok(account)
    }

    /// Commits verified Google ownership, explicit device binding and any confirmed guest retirement.
    ///
    /// # Errors
    /// Rejects changed authority, another linked account, unconfirmed/forbidden retirement,
    /// or any failure before the entire identity transition commits.
    pub async fn connect_google_account(
        &self,
        identity: &AccountIdentity,
        subject_fingerprint: &[u8; 32],
        discard_guest: bool,
    ) -> Result<GoogleAccountLink, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let current = revalidate_account_identity(&mut transaction, identity).await?;
        let linked_id: Option<String> =
            sqlx::query_scalar("SELECT user_id FROM google_accounts WHERE subject_fingerprint = ?")
                .bind(subject_fingerprint.as_slice())
                .fetch_optional(&mut *transaction)
                .await?;
        let mut events = Vec::new();
        let mut revoked_session_fingerprints = Vec::new();
        let identity_switched = match (&current, &linked_id) {
            (Some(current), Some(target)) if current.user_id != *target => {
                if !discard_guest {
                    return Err(rejected(
                        "account_switch_confirmation_required",
                        "Confirm guest discard before switching to the existing account.",
                    ));
                }
                (events, revoked_session_fingerprints) =
                    crate::account_guest_retirement::retire_guest(
                        &mut transaction,
                        current,
                        identity.authority.browser_fingerprint(),
                    )
                    .await?;
                true
            }
            _ => false,
        };
        let user = match linked_id {
            Some(user_id) => load_account_user(&mut transaction, &user_id).await?,
            None => match current {
                Some(user) => user,
                None => create_account_user(&mut transaction, identity).await?,
            },
        };
        let existing = account_fingerprint(&mut transaction, &user.user_id).await?;
        if existing.is_some_and(|fingerprint| fingerprint != *subject_fingerprint) {
            return Err(rejected(
                "account_link_conflict",
                "This identity is already connected to another Google account.",
            ));
        }
        if let Some(device) = identity.authority.browser_fingerprint() {
            bind_account_device(&mut transaction, &user.user_id, device).await?;
        }
        if existing.is_none() {
            sqlx::query("INSERT INTO google_accounts(subject_fingerprint, user_id) VALUES (?, ?)")
                .bind(subject_fingerprint.as_slice())
                .bind(&user.user_id)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(GoogleAccountLink {
            account: account_view(subject_fingerprint),
            user,
            identity_switched,
            events,
            revoked_session_fingerprints,
        })
    }

    /// Removes only the Google link; profiles, devices and memberships remain unchanged.
    ///
    /// # Errors
    /// Rejects missing/changed identity or a persistence failure.
    pub async fn disconnect_google_account(
        &self,
        identity: &AccountIdentity,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let user = revalidate_account_identity(&mut transaction, identity)
            .await?
            .ok_or_else(|| {
                rejected(
                    "device_identity_required",
                    "A server identity is required to disconnect Google.",
                )
            })?;
        sqlx::query("DELETE FROM google_accounts WHERE user_id = ?")
            .bind(&user.user_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

pub(crate) async fn account_fingerprint(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
) -> Result<Option<[u8; 32]>, PersistenceError> {
    let value: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT subject_fingerprint FROM google_accounts WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&mut **transaction)
            .await?;
    value
        .map(|bytes| {
            bytes.try_into().map_err(|_| {
                rejected(
                    "invalid_state",
                    "Stored Google account fingerprint is invalid.",
                )
            })
        })
        .transpose()
}

fn account_view(fingerprint: &[u8; 32]) -> GoogleAccount {
    let namespace = Uuid::new_v5(&Uuid::NAMESPACE_URL, b"https://agentsassemble.app/accounts");
    GoogleAccount {
        account_id: format!(
            "acct-{}",
            Uuid::new_v5(&namespace, hex::encode(fingerprint).as_bytes())
        ),
    }
}

async fn create_account_user(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: &AccountIdentity,
) -> Result<AccountUser, PersistenceError> {
    if identity.authority.browser_fingerprint().is_none() {
        return Err(rejected(
            "device_identity_required",
            "A browser credential is required to create an account.",
        ));
    }
    let suffix = Uuid::new_v4().simple().to_string();
    let user = AccountUser {
        user_id: format!("u-{suffix}"),
        participant_id: format!("person-{suffix}"),
        profile: UserProfile::for_admitted_human("Google 사용자", "", Utc::now())
            .ok_or_else(|| rejected("invalid_state", "Cannot initialize the account profile."))?,
    };
    sqlx::query(
        "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(&user.participant_id)
    .bind(serde_json::to_string(&user.profile)?)
    .execute(&mut **transaction)
    .await?;
    Ok(user)
}

async fn bind_account_device(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    device: &[u8; 32],
) -> Result<(), PersistenceError> {
    match device_user_id(transaction, device).await? {
        Some(current) if current != user_id => {
            return Err(rejected(
                "account_device_mismatch",
                "The browser credential belongs to another identity.",
            ));
        }
        Some(_) => (),
        None => {
            sqlx::query("INSERT INTO human_device_credentials(credential_fingerprint, user_id, created_at) VALUES (?, ?, ?)").bind(device.as_slice()).bind(user_id).bind(Utc::now().timestamp_micros()).execute(&mut **transaction).await?;
        }
    }
    Ok(())
}
