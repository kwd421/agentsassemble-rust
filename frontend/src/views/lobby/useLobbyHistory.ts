import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type UIEvent,
} from "react";

import {
  mergeLobbyEvents,
  type LobbyEvent,
} from "../../api";
import type { RoomDockItem } from "../../lib/roomDockModel";
import {
  applyCanonicalParticipantProfiles,
  type CanonicalParticipantProfile,
} from "../../lib/canonicalRoomProjection";
import type { RoomTypingIndicator } from "../../lib/roomTypingIndicators";
import { isVoteTransitionKind } from "../../lib/voteEventKind";


const HISTORY_TOP_THRESHOLD = 120;
const INITIAL_HISTORY_MESSAGE_TARGET = 20;


function feedIsNearBottom(element: HTMLDivElement) {
  const { scrollHeight, scrollTop, clientHeight } = element;
  return scrollHeight - scrollTop - clientHeight <= 64;
}


type CanonicalHistoryPage = {
  loadedCount: number;
  oldestSeq: number;
  hasMoreBefore: boolean;
  events?: LobbyEvent[];
};

type HistoryPageRequest = {
  roomId: string;
  windowRevision: number;
};

function mergeDisplayedLobbyEvents(
  existing: LobbyEvent[],
  incoming: LobbyEvent[],
) {
  const retained = existing.filter((event) => !isVoteTransitionKind(event.kind));
  const durableIncoming = incoming.filter(
    (event) => !isVoteTransitionKind(event.kind),
  );
  const merged = mergeLobbyEvents(retained, durableIncoming).sort((left, right) => {
    const sequence = Number(left.seq || 0) - Number(right.seq || 0);
    return sequence || left.created_at.localeCompare(right.created_at);
  });
  return merged.length === existing.length &&
    merged.every((event, index) => event === existing[index])
    ? existing
    : merged;
}

function reconcileFixedHistoryWindow(
  displayed: LobbyEvent[],
  canonical: LobbyEvent[],
) {
  const displayedRecordIds = new Set(
    displayed.map((event) => event.record_id).filter(Boolean),
  );
  const relevant = canonical.filter((event) =>
    Boolean(event.record_id && displayedRecordIds.has(event.record_id)) ||
    Boolean(
      event.target_event_id &&
      ["message_updated", "message_deleted"].includes(event.flow_action || "") &&
      displayedRecordIds.has(event.target_event_id),
    ));
  return mergeDisplayedLobbyEvents(displayed, relevant);
}

function reconcileDisplayedVoteRevisions(
  previous: Record<string, string>,
  displayed: LobbyEvent[],
  canonical: LobbyEvent[],
  roomId: string,
) {
  const displayedVoteIds = new Set(
    displayed
      .filter((event) => event.kind === "vote" && !event.message_deleted)
      .map((event) => event.vote_id || event.record_id || event.id)
      .filter(Boolean),
  );
  const next: Record<string, string> = {};
  displayedVoteIds.forEach((voteId) => {
    if (previous[voteId]) next[voteId] = previous[voteId];
  });
  canonical.forEach((event) => {
    if (
      isVoteTransitionKind(event.kind) &&
      event.vote_id &&
      displayedVoteIds.has(event.vote_id) &&
      (!event.flow_meeting_id || event.flow_meeting_id === roomId)
    ) {
      next[event.vote_id] = event.id;
    }
  });
  const previousKeys = Object.keys(previous);
  const nextKeys = Object.keys(next);
  return previousKeys.length === nextKeys.length &&
    nextKeys.every((key) => previous[key] === next[key])
    ? previous
    : next;
}


export function useLobbyHistory({
  activeRoom,
  typingIndicators,
  canonicalEvents,
  canonicalHistoryReady,
  canonicalOldestSeq,
  canonicalHasMoreHistory,
  canonicalWindowRevision,
  participantProfiles = {},
  loadCanonicalHistory,
}: {
  activeRoom: RoomDockItem;
  typingIndicators: RoomTypingIndicator[];
  canonicalEvents?: LobbyEvent[];
  canonicalHistoryReady: boolean;
  canonicalOldestSeq: number;
  canonicalHasMoreHistory: boolean;
  canonicalWindowRevision: number;
  participantProfiles?: Record<string, CanonicalParticipantProfile>;
  loadCanonicalHistory?: (beforeSeq: number) => Promise<CanonicalHistoryPage>;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedToLatestRef = useRef(true);
  const historyReadyRef = useRef(false);
  const historyRoomRef = useRef(activeRoom.id);
  const initialBackfillFailedRoomRef = useRef("");
  const loadingHistoryRequestRef = useRef<HistoryPageRequest | null>(null);
  const historyLoadSuppressedUntilRef = useRef(0);
  const historyWindowActiveRef = useRef(false);
  const canonicalWindowRevisionRef = useRef(-1);
  const currentCanonicalWindowRevisionRef = useRef(canonicalWindowRevision);
  const canonicalOldestSeqRef = useRef(canonicalOldestSeq);
  const prependAnchorRef = useRef<{
    roomId: string;
    windowRevision: number;
    eventId: string;
    viewportTop: number;
    scrollHeight: number;
    scrollTop: number;
  } | null>(null);
  const [events, setEvents] = useState<LobbyEvent[]>([]);
  const [loadedRoomId, setLoadedRoomId] = useState("");
  const [pinnedToLatest, setPinnedToLatest] = useState(true);
  const [hasMoreHistory, setHasMoreHistory] = useState(true);
  const [oldestSeq, setOldestSeq] = useState(canonicalOldestSeq);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [historyLoadError, setHistoryLoadError] = useState(false);
  const [historyWindowActive, setHistoryWindowActive] = useState(false);
  const [displayedVoteRevisions, setDisplayedVoteRevisions] = useState<
    Record<string, string>
  >({});
  const loaded = loadedRoomId === activeRoom.id;
  currentCanonicalWindowRevisionRef.current = canonicalWindowRevision;
  if (historyRoomRef.current !== activeRoom.id) {
    historyRoomRef.current = activeRoom.id;
    historyReadyRef.current = false;
    initialBackfillFailedRoomRef.current = "";
    prependAnchorRef.current = null;
    historyWindowActiveRef.current = false;
    canonicalWindowRevisionRef.current = -1;
    canonicalOldestSeqRef.current = canonicalOldestSeq;
    loadingHistoryRequestRef.current = null;
  }

  const visibleEvents = useMemo(() => {
    const roomEvents = applyCanonicalParticipantProfiles(events, participantProfiles)
      .filter(
        (event) =>
          !isVoteTransitionKind(event.kind) &&
          (!event.flow_meeting_id ||
            event.flow_meeting_id === activeRoom.meetingId)
      );
    return roomEvents;
  }, [
    activeRoom.meetingId,
    events,
    participantProfiles,
  ]);
  useEffect(() => {
    setDisplayedVoteRevisions((previous) =>
      reconcileDisplayedVoteRevisions(
        previous,
        events,
        canonicalEvents || [],
        activeRoom.meetingId,
      ));
  }, [activeRoom.meetingId, canonicalEvents, events]);

  const updatePinnedToLatest = useCallback((nextPinned: boolean) => {
    pinnedToLatestRef.current = nextPinned;
    setPinnedToLatest(nextPinned);
  }, []);

  const retireHistoryPageRequest = useCallback(() => {
    loadingHistoryRequestRef.current = null;
    prependAnchorRef.current = null;
    setLoadingOlder(false);
    setHistoryLoadError(false);
  }, []);

  const scrollToLatest = useCallback(() => {
    retireHistoryPageRequest();
    historyWindowActiveRef.current = false;
    setHistoryWindowActive(false);
    setDisplayedVoteRevisions({});
    setEvents(mergeDisplayedLobbyEvents([], canonicalEvents || []));
    setOldestSeq(canonicalOldestSeq);
    setHasMoreHistory(canonicalHasMoreHistory);
    const element = scrollRef.current;
    if (!element) return;
    window.requestAnimationFrame(() => {
      element.scrollTop = element.scrollHeight;
    });
    updatePinnedToLatest(true);
  }, [
    canonicalEvents,
    canonicalHasMoreHistory,
    canonicalOldestSeq,
    retireHistoryPageRequest,
    updatePinnedToLatest,
  ]);

  const historyPageRequestIsCurrent = useCallback((request: HistoryPageRequest) =>
    historyRoomRef.current === request.roomId &&
    currentCanonicalWindowRevisionRef.current === request.windowRevision &&
    loadingHistoryRequestRef.current === request, []);

  const acceptHistoryPage = useCallback((
    request: HistoryPageRequest,
    page: CanonicalHistoryPage,
  ) => {
    if (!historyPageRequestIsCurrent(request)) return false;
    setOldestSeq(page.oldestSeq);
    setHasMoreHistory(page.hasMoreBefore);
    if (page.events?.length) {
      setEvents((previous) =>
        mergeDisplayedLobbyEvents(previous, page.events || [])
      );
    }
    return true;
  }, [historyPageRequestIsCurrent]);

  const loadOlderHistory = useCallback((triggerScrollTop?: number) => {
    if (
      Date.now() < historyLoadSuppressedUntilRef.current ||
      !historyReadyRef.current ||
      loadingHistoryRequestRef.current?.roomId === activeRoom.id ||
      !hasMoreHistory ||
      !loaded
    ) {
      return;
    }
    const element = scrollRef.current;
    if (!element || !loadCanonicalHistory) return;
    const request: HistoryPageRequest = {
      roomId: activeRoom.id,
      windowRevision: currentCanonicalWindowRevisionRef.current,
    };
    loadingHistoryRequestRef.current = request;
    setLoadingOlder(true);
    setHistoryLoadError(false);
    const anchorEventId = visibleEvents[0]?.id || "";
    const anchorElement = Array.from(
      element.querySelectorAll<HTMLElement>("[data-room-event-id]")
    ).find((candidate) => candidate.dataset.roomEventId === anchorEventId);
    prependAnchorRef.current = {
      roomId: request.roomId,
      windowRevision: request.windowRevision,
      eventId: anchorEventId,
      viewportTop: anchorElement
        ? element.getBoundingClientRect().top +
          anchorElement.offsetTop -
          (triggerScrollTop ?? element.scrollTop)
        : element.getBoundingClientRect().top,
      scrollHeight: element.scrollHeight,
      scrollTop: triggerScrollTop ?? element.scrollTop,
    };
    loadCanonicalHistory(oldestSeq)
        .then((page) => {
          if (!acceptHistoryPage(request, page)) return;
          if (!page.loadedCount) {
            prependAnchorRef.current = null;
          }
        })
        .catch(() => {
          if (!historyPageRequestIsCurrent(request)) return;
          prependAnchorRef.current = null;
          setHistoryLoadError(true);
        })
        .finally(() => {
          if (loadingHistoryRequestRef.current !== request) return;
          loadingHistoryRequestRef.current = null;
          if (
            historyRoomRef.current === request.roomId &&
            currentCanonicalWindowRevisionRef.current === request.windowRevision
          ) {
            setLoadingOlder(false);
          }
        });
  }, [
    activeRoom.id,
    activeRoom.meetingId,
    acceptHistoryPage,
    events,
    hasMoreHistory,
    loadCanonicalHistory,
    loaded,
    oldestSeq,
    visibleEvents,
    historyPageRequestIsCurrent,
  ]);

  const handleLobbyScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      if (historyWindowActiveRef.current) {
        updatePinnedToLatest(false);
        return;
      }
      updatePinnedToLatest(feedIsNearBottom(event.currentTarget));
      if (
        Date.now() >= historyLoadSuppressedUntilRef.current
        && event.currentTarget.scrollTop <= HISTORY_TOP_THRESHOLD
      ) {
        loadOlderHistory(event.currentTarget.scrollTop);
      }
    },
    [loadOlderHistory, updatePinnedToLatest]
  );

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element || !loaded) return;
    const anchor = prependAnchorRef.current;
    if (
      anchor &&
      (anchor.roomId !== activeRoom.id ||
        anchor.windowRevision !== currentCanonicalWindowRevisionRef.current)
    ) {
      prependAnchorRef.current = null;
    } else if (anchor) {
      prependAnchorRef.current = null;
      const restoreAnchor = () => {
        const anchorElement = Array.from(
          element.querySelectorAll<HTMLElement>("[data-room-event-id]")
        ).find((candidate) => candidate.dataset.roomEventId === anchor.eventId);
        if (anchorElement && anchor.eventId) {
          element.scrollTop +=
            anchorElement.getBoundingClientRect().top - anchor.viewportTop;
          return;
        }
        element.scrollTop =
          element.scrollHeight - anchor.scrollHeight + anchor.scrollTop;
      };
      restoreAnchor();
      window.requestAnimationFrame(restoreAnchor);
      return;
    }
    if (pinnedToLatestRef.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [activeRoom.id, loaded, typingIndicators, visibleEvents]);

  useEffect(() => {
    if (!loaded || !hasMoreHistory || loadingOlder) return;
    const scheduledRoomId = activeRoom.id;
    const timeoutId = window.setTimeout(() => {
      if (historyRoomRef.current !== scheduledRoomId) return;
      const element = scrollRef.current;
      if (!element) return;
      historyReadyRef.current = true;
      if (element.scrollHeight <= element.clientHeight + HISTORY_TOP_THRESHOLD) {
        loadOlderHistory(element.scrollTop);
      }
    }, 50);
    return () => window.clearTimeout(timeoutId);
  }, [
    activeRoom.id,
    hasMoreHistory,
    loadOlderHistory,
    loaded,
    loadingOlder,
    visibleEvents,
  ]);

  useEffect(() => {
    updatePinnedToLatest(true);
    setHistoryLoadError(false);
    setHistoryWindowActive(false);
    setDisplayedVoteRevisions({});
  }, [activeRoom.id, updatePinnedToLatest]);

  useEffect(() => {
    const windowChanged =
      canonicalWindowRevisionRef.current !== canonicalWindowRevision;
    const enteringCanonicalWindow = windowChanged || !loaded;
    const requestOldestSeq = enteringCanonicalWindow ? canonicalOldestSeq : oldestSeq;
    const displayedForBackfill = enteringCanonicalWindow
      ? canonicalEvents || []
      : events;
    const canBackfill = enteringCanonicalWindow
      ? canonicalHasMoreHistory
      : hasMoreHistory;
    if (windowChanged) {
      canonicalWindowRevisionRef.current = canonicalWindowRevision;
      retireHistoryPageRequest();
      initialBackfillFailedRoomRef.current = "";
      historyWindowActiveRef.current = false;
      setHistoryWindowActive(false);
      setDisplayedVoteRevisions({});
      setEvents(mergeDisplayedLobbyEvents([], canonicalEvents || []));
      setOldestSeq(canonicalOldestSeq);
      canonicalOldestSeqRef.current = canonicalOldestSeq;
      setHasMoreHistory(canonicalHasMoreHistory);
    } else if (historyWindowActiveRef.current && historyRoomRef.current === activeRoom.id) {
      setEvents((previous) =>
        reconcileFixedHistoryWindow(previous, canonicalEvents || []));
      setHasMoreHistory(false);
      setLoadedRoomId(activeRoom.id);
      return;
    } else {
      const priorCanonicalOldest = canonicalOldestSeqRef.current;
      canonicalOldestSeqRef.current = canonicalOldestSeq;
      if (
        canonicalOldestSeq > priorCanonicalOldest &&
        oldestSeq >= priorCanonicalOldest
      ) {
        setOldestSeq(canonicalOldestSeq);
        setHasMoreHistory(true);
      }
      setEvents((previous) =>
        mergeDisplayedLobbyEvents(
          previous,
          canonicalEvents || [],
        )
      );
    }
    if (!canonicalHistoryReady) {
        historyReadyRef.current = false;
        if (loadingHistoryRequestRef.current?.roomId !== activeRoom.id) {
          setLoadingOlder(false);
        }
        setLoadedRoomId((current) => (current === activeRoom.id ? "" : current));
      return;
    }
    const needsInitialBackfill =
        canBackfill &&
        displayedForBackfill.filter((event) => !isVoteTransitionKind(event.kind)).length <
          INITIAL_HISTORY_MESSAGE_TARGET &&
        initialBackfillFailedRoomRef.current !== activeRoom.id;
    if (needsInitialBackfill && loadCanonicalHistory) {
        historyReadyRef.current = false;
        setLoadedRoomId((current) => (current === activeRoom.id ? "" : current));
        if (loadingHistoryRequestRef.current?.roomId !== activeRoom.id) {
          const request: HistoryPageRequest = {
            roomId: activeRoom.id,
            windowRevision: currentCanonicalWindowRevisionRef.current,
          };
          loadingHistoryRequestRef.current = request;
          setLoadingOlder(true);
          void loadCanonicalHistory(requestOldestSeq)
            .then((page) => {
              if (!acceptHistoryPage(request, page)) return;
              historyReadyRef.current = true;
              setLoadedRoomId(request.roomId);
            })
            .catch(() => {
              if (!historyPageRequestIsCurrent(request)) return;
              initialBackfillFailedRoomRef.current = request.roomId;
              setHistoryLoadError(true);
              historyReadyRef.current = true;
              setLoadedRoomId(request.roomId);
            })
            .finally(() => {
              if (loadingHistoryRequestRef.current !== request) return;
              loadingHistoryRequestRef.current = null;
              if (
                historyRoomRef.current === request.roomId &&
                currentCanonicalWindowRevisionRef.current === request.windowRevision
              ) {
                setLoadingOlder(false);
              }
            });
        }
      return;
    }
    historyReadyRef.current = true;
    if (loadingHistoryRequestRef.current?.roomId !== activeRoom.id) {
      setLoadingOlder(false);
    }
    setLoadedRoomId(activeRoom.id);
  }, [
    activeRoom.meetingId,
    activeRoom.id,
    acceptHistoryPage,
    canonicalEvents,
    canonicalHasMoreHistory,
    canonicalHistoryReady,
    canonicalOldestSeq,
    canonicalWindowRevision,
    events,
    hasMoreHistory,
    historyPageRequestIsCurrent,
    loadCanonicalHistory,
    loaded,
    oldestSeq,
    retireHistoryPageRequest,
  ]);

  const handleLobbyPosted = useCallback((postedEvents: LobbyEvent[]) => {
    retireHistoryPageRequest();
    historyWindowActiveRef.current = false;
    setHistoryWindowActive(false);
    setDisplayedVoteRevisions({});
    setEvents(mergeDisplayedLobbyEvents(canonicalEvents || [], postedEvents));
    setOldestSeq(canonicalOldestSeq);
    setHasMoreHistory(canonicalHasMoreHistory);
  }, [
    canonicalEvents,
    canonicalHasMoreHistory,
    canonicalOldestSeq,
    retireHistoryPageRequest,
  ]);

  const showHistoryWindow = useCallback((historyEvents: LobbyEvent[]) => {
    retireHistoryPageRequest();
    historyWindowActiveRef.current = true;
    setHistoryWindowActive(true);
    setDisplayedVoteRevisions({});
    setEvents(mergeDisplayedLobbyEvents([], historyEvents));
    setHasMoreHistory(false);
    setLoadedRoomId(activeRoom.id);
    updatePinnedToLatest(false);
  }, [activeRoom.id, retireHistoryPageRequest, updatePinnedToLatest]);

  const suppressAutomaticHistoryLoad = useCallback((durationMs = 1200) => {
    historyLoadSuppressedUntilRef.current = Date.now() + durationMs;
  }, []);

  return {
    handleLobbyPosted,
    handleLobbyScroll,
    showHistoryWindow,
    suppressAutomaticHistoryLoad,
    hasMoreHistory,
    historyLoadError,
    historyWindowActive,
    loadOlderHistory,
    loaded,
    loadingOlder,
    pinnedToLatest,
    scrollRef,
    scrollToLatest,
    voteRevisions: displayedVoteRevisions,
    visibleEvents,
  };
}
