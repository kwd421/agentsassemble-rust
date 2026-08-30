import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import {
  fetchRoomMessageContext,
  searchRoomMessages,
  type MessageSearchAuthority,
  type RoomSearchResult,
} from "../api";

export function useRoomMessageSearch({
  roomId,
  channelId,
  authority,
}: {
  roomId: string;
  channelId: string;
  authority?: MessageSearchAuthority;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<RoomSearchResult[]>([]);
  const [nextCursor, setNextCursor] = useState("");
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState("");
  const requestVersionRef = useRef(0);
  const authorityKind = authority?.kind || "unavailable";
  const authorityToken = authority?.kind === "remote" ? authority.sessionToken : "";

  const updateQuery = useCallback((value: string) => {
    requestVersionRef.current += 1;
    setQuery(value);
    setResults([]);
    setNextCursor("");
    setLoadingMore(false);
    setError("");
  }, []);

  useLayoutEffect(() => {
    requestVersionRef.current += 1;
    setQuery("");
    setResults([]);
    setNextCursor("");
    setLoading(false);
    setLoadingMore(false);
    setError("");
    return () => {
      requestVersionRef.current += 1;
    };
  }, [authorityKind, authorityToken, channelId, roomId]);

  useEffect(() => {
    const cleanQuery = query.trim();
    const version = ++requestVersionRef.current;
    if (!cleanQuery) {
      setLoading(false);
      return undefined;
    }
    if (!authority) {
      setLoading(false);
      setError("이 환경에서는 로비 메시지 검색을 사용할 수 없습니다.");
      return undefined;
    }
    setLoading(true);
    const timer = window.setTimeout(() => {
      void searchRoomMessages({
        roomId,
        channelId,
        query: cleanQuery,
        authority,
        beforeDispatch: () => {
          if (requestVersionRef.current !== version) {
            throw new Error("메시지 검색 요청 권위가 변경되었습니다.");
          }
        },
      })
        .then((page) => {
          if (requestVersionRef.current !== version) return;
          setResults(page.results);
          setNextCursor(page.next_cursor);
          setError("");
        })
        .catch((reason) => {
          if (requestVersionRef.current !== version) return;
          setResults([]);
          setNextCursor("");
          setError(reason instanceof Error ? reason.message : "메시지를 검색하지 못했습니다.");
        })
        .finally(() => {
          if (requestVersionRef.current === version) setLoading(false);
        });
    }, 250);
    return () => window.clearTimeout(timer);
  }, [authorityKind, authorityToken, channelId, query, roomId]);

  const loadMore = useCallback(async () => {
    const cleanQuery = query.trim();
    if (!cleanQuery || !nextCursor || loadingMore || !authority) return;
    const version = requestVersionRef.current;
    setLoadingMore(true);
    try {
      const page = await searchRoomMessages({
        roomId,
        channelId,
        query: cleanQuery,
        cursor: nextCursor,
        authority,
        beforeDispatch: () => {
          if (requestVersionRef.current !== version) {
            throw new Error("메시지 검색 요청 권위가 변경되었습니다.");
          }
        },
      });
      if (requestVersionRef.current !== version) return;
      setResults((current) => [...current, ...page.results]);
      setNextCursor(page.next_cursor);
      setError("");
    } catch (reason) {
      if (requestVersionRef.current !== version) return;
      setError(reason instanceof Error ? reason.message : "검색 결과를 더 불러오지 못했습니다.");
    } finally {
      if (requestVersionRef.current === version) setLoadingMore(false);
    }
  }, [authority, channelId, loadingMore, nextCursor, query, roomId]);

  const readContext = useCallback(async (eventId: string) => {
    if (!authority) {
      throw new Error("이 환경에서는 로비 메시지 검색을 사용할 수 없습니다.");
    }
    const version = requestVersionRef.current;
    try {
      const context = await fetchRoomMessageContext({
        roomId,
        channelId,
        eventId,
        authority,
        beforeDispatch: () => {
          if (requestVersionRef.current !== version) {
            throw new Error("메시지 검색 요청 권위가 변경되었습니다.");
          }
        },
      });
      return requestVersionRef.current === version ? context : null;
    } catch (reason) {
      if (requestVersionRef.current !== version) return null;
      throw reason;
    }
  }, [authority, channelId, roomId]);

  return {
    error,
    hasMore: Boolean(nextCursor),
    loading,
    loadingMore,
    loadMore,
    query,
    readContext,
    results,
    setError,
    updateQuery,
  };
}

export type RoomMessageSearchController = ReturnType<typeof useRoomMessageSearch>;
