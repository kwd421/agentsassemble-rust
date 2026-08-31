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
  mergeLobbyEventsByCreatedAt,
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

function mergeDisplayedLobbyEvents(
  existing: LobbyEvent[],
  incoming: LobbyEvent[],
) {
  const merged = mergeLobbyEvents(existing, incoming).sort((left, right) => {
    const sequence = Number(left.seq || 0) - Number(right.seq || 0);
    return sequence || left.created_at.localeCompare(right.created_at);
  });
  return merged.length === existing.length &&
    merged.every((event, index) => event === existing[index])
    ? existing
    : merged;
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
  const loadingOlderRoomRef = useRef("");
  const historyLoadSuppressedUntilRef = useRef(0);
  const historyWindowActiveRef = useRef(false);
  const canonicalWindowRevisionRef = useRef(-1);
  const canonicalOldestSeqRef = useRef(canonicalOldestSeq);
  const historicalVoteIdsRef = useRef<Set<string>>(new Set());
  const prependAnchorRef = useRef<{
    roomId: string;
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
  const [historicalVoteRevisions, setHistoricalVoteRevisions] = useState<
    Record<string, string>
  >({});
  const loaded = loadedRoomId === activeRoom.id;
  if (historyRoomRef.current !== activeRoom.id) {
    historyRoomRef.current = activeRoom.id;
    historyReadyRef.current = false;
    initialBackfillFailedRoomRef.current = "";
    prependAnchorRef.current = null;
    historyWindowActiveRef.current = false;
    historicalVoteIdsRef.current.clear();
    canonicalWindowRevisionRef.current = -1;
    canonicalOldestSeqRef.current = canonicalOldestSeq;
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
  const voteRevisions = useMemo(() => {
    const revisions: Record<string, string> = {};
    events.forEach((event) => {
      if (
        !isVoteTransitionKind(event.kind) ||
        !event.vote_id ||
        (event.flow_meeting_id && event.flow_meeting_id !== activeRoom.meetingId)
      ) {
        return;
      }
      revisions[event.vote_id] = event.id;
    });
    return historyWindowActive
      ? { ...revisions, ...historicalVoteRevisions }
      : revisions;
  }, [activeRoom.meetingId, events, historicalVoteRevisions, historyWindowActive]);

  const updatePinnedToLatest = useCallback((nextPinned: boolean) => {
    pinnedToLatestRef.current = nextPinned;
    setPinnedToLatest(nextPinned);
  }, []);

  const scrollToLatest = useCallback(() => {
    historyWindowActiveRef.current = false;
    historicalVoteIdsRef.current.clear();
    setHistoryWindowActive(false);
    setHistoricalVoteRevisions({});
    setEvents(mergeDisplayedLobbyEvents([], canonicalEvents || []));
    setOldestSeq(canonicalOldestSeq);
    setHasMoreHistory(canonicalHasMoreHistory);
    const element = scrollRef.current;
    if (!element) return;
    window.requestAnimationFrame(() => {
      element.scrollTop = element.scrollHeight;
    });
    updatePinnedToLatest(true);
  }, [canonicalEvents, canonicalHasMoreHistory, canonicalOldestSeq, updatePinnedToLatest]);

  const acceptHistoryPage = useCallback((
    requestedRoomId: string,
    page: CanonicalHistoryPage,
  ) => {
    if (historyRoomRef.current !== requestedRoomId) return;
    setOldestSeq(page.oldestSeq);
    setHasMoreHistory(page.hasMoreBefore);
    if (page.events?.length) {
      setEvents((previous) =>
        mergeDisplayedLobbyEvents(previous, page.events || [])
      );
    }
  }, []);

  const loadOlderHistory = useCallback((triggerScrollTop?: number) => {
    if (
      Date.now() < historyLoadSuppressedUntilRef.current ||
      !historyReadyRef.current ||
      loadingOlderRoomRef.current === activeRoom.id ||
      !hasMoreHistory ||
      !loaded
    ) {
      return;
    }
    const element = scrollRef.current;
    if (!element || !loadCanonicalHistory) return;
    const requestedRoomId = activeRoom.id;
    loadingOlderRoomRef.current = requestedRoomId;
    setLoadingOlder(true);
    setHistoryLoadError(false);
    const anchorEventId = visibleEvents[0]?.id || "";
    const anchorElement = Array.from(
      element.querySelectorAll<HTMLElement>("[data-room-event-id]")
    ).find((candidate) => candidate.dataset.roomEventId === anchorEventId);
    prependAnchorRef.current = {
      roomId: requestedRoomId,
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
          if (historyRoomRef.current !== requestedRoomId) return;
          acceptHistoryPage(requestedRoomId, page);
          if (!page.loadedCount) {
            prependAnchorRef.current = null;
            if (loadingOlderRoomRef.current === requestedRoomId) {
              loadingOlderRoomRef.current = "";
            }
          }
        })
        .catch(() => {
          if (historyRoomRef.current !== requestedRoomId) return;
          prependAnchorRef.current = null;
          setHistoryLoadError(true);
          if (loadingOlderRoomRef.current === requestedRoomId) {
            loadingOlderRoomRef.current = "";
          }
        })
        .finally(() => {
          if (historyRoomRef.current === requestedRoomId) {
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
    if (anchor?.roomId === activeRoom.id) {
      prependAnchorRef.current = null;
      if (loadingOlderRoomRef.current === activeRoom.id) {
        loadingOlderRoomRef.current = "";
      }
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
    setHistoricalVoteRevisions({});
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
      historyWindowActiveRef.current = false;
      historicalVoteIdsRef.current.clear();
      setHistoryWindowActive(false);
      setHistoricalVoteRevisions({});
      setEvents(mergeDisplayedLobbyEvents([], canonicalEvents || []));
      setOldestSeq(canonicalOldestSeq);
      canonicalOldestSeqRef.current = canonicalOldestSeq;
      setHasMoreHistory(canonicalHasMoreHistory);
    } else if (historyWindowActiveRef.current && historyRoomRef.current === activeRoom.id) {
      const latest: Record<string, string> = {};
      (canonicalEvents || []).forEach((event) => {
        if (
          isVoteTransitionKind(event.kind) &&
          event.vote_id &&
          historicalVoteIdsRef.current.has(event.vote_id) &&
          (!event.flow_meeting_id || event.flow_meeting_id === activeRoom.meetingId)
        ) {
          latest[event.vote_id] = event.id;
        }
      });
      if (Object.keys(latest).length) {
        setHistoricalVoteRevisions((previous) => ({ ...previous, ...latest }));
      }
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
        if (loadingOlderRoomRef.current !== activeRoom.id) {
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
        if (loadingOlderRoomRef.current !== activeRoom.id) {
          const requestedRoomId = activeRoom.id;
          loadingOlderRoomRef.current = requestedRoomId;
          setLoadingOlder(true);
          void loadCanonicalHistory(requestOldestSeq)
            .then((page) => {
              if (historyRoomRef.current !== requestedRoomId) return;
              acceptHistoryPage(requestedRoomId, page);
              historyReadyRef.current = true;
              setLoadedRoomId(requestedRoomId);
            })
            .catch(() => {
              if (historyRoomRef.current !== requestedRoomId) return;
              initialBackfillFailedRoomRef.current = requestedRoomId;
              setHistoryLoadError(true);
              historyReadyRef.current = true;
              setLoadedRoomId(requestedRoomId);
            })
            .finally(() => {
              if (loadingOlderRoomRef.current === requestedRoomId) {
                loadingOlderRoomRef.current = "";
              }
              if (historyRoomRef.current === requestedRoomId) {
                setLoadingOlder(false);
              }
            });
        }
      return;
    }
    historyReadyRef.current = true;
    if (loadingOlderRoomRef.current !== activeRoom.id) {
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
    loadCanonicalHistory,
    loaded,
    oldestSeq,
  ]);

  const handleLobbyPosted = useCallback((postedEvents: LobbyEvent[]) => {
    historyWindowActiveRef.current = false;
    historicalVoteIdsRef.current.clear();
    setHistoryWindowActive(false);
    setHistoricalVoteRevisions({});
    setEvents(mergeDisplayedLobbyEvents(canonicalEvents || [], postedEvents));
    setOldestSeq(canonicalOldestSeq);
    setHasMoreHistory(canonicalHasMoreHistory);
  }, [canonicalEvents, canonicalHasMoreHistory, canonicalOldestSeq]);

  const showHistoryWindow = useCallback((historyEvents: LobbyEvent[]) => {
    historyWindowActiveRef.current = true;
    historicalVoteIdsRef.current = new Set(
      historyEvents
        .filter(
          (event) =>
            event.kind === "vote" &&
            (!event.flow_meeting_id || event.flow_meeting_id === activeRoom.meetingId)
        )
        .map((event) => event.vote_id || event.id)
        .filter(Boolean)
    );
    setHistoryWindowActive(true);
    setHistoricalVoteRevisions({});
    setEvents(mergeLobbyEventsByCreatedAt([], historyEvents));
    setHasMoreHistory(false);
    setLoadedRoomId(activeRoom.id);
    updatePinnedToLatest(false);
  }, [activeRoom.id, activeRoom.meetingId, updatePinnedToLatest]);

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
    voteRevisions,
    visibleEvents,
  };
}
