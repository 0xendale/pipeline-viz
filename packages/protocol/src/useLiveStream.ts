import { useEffect } from "react";
import type { ServerMessage } from "./types";
import { usePipelineStore } from "./store";
import { nextBackoffMs } from "./backoff";

function socketUrl(): string {
  const scheme = window.location.protocol === "https:" ? "wss" : "ws";
  return `${scheme}://${window.location.host}/ws`;
}

/**
 * Holds a WebSocket open for the life of the app, reconnecting when it closes.
 *
 * The server closes clients that fall behind rather than sending them a stream
 * with a gap in it, so a disconnect is an expected event, not an error.
 * Reconnecting fetches a fresh snapshot, which is the recovery path the
 * protocol was designed around.
 */
export function useLiveStream(): void {
  const applyMessage = usePipelineStore((state) => state.applyMessage);
  const setConnection = usePipelineStore((state) => state.setConnection);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let retryTimer: number | undefined;
    let attempt = 0;
    let unmounted = false;

    const connect = () => {
      if (unmounted) return;
      socket = new WebSocket(socketUrl());

      socket.onopen = () => {
        attempt = 0;
        setConnection("live");
      };

      socket.onmessage = (event) => {
        try {
          applyMessage(JSON.parse(event.data as string) as ServerMessage);
        } catch {
          // An unparseable frame means a version mismatch, not a reason to tear
          // down a working connection. Skip it.
        }
      };

      socket.onclose = () => {
        if (unmounted) return;
        setConnection("reconnecting");
        retryTimer = window.setTimeout(connect, nextBackoffMs(attempt));
        attempt += 1;
      };

      socket.onerror = () => socket?.close();
    };

    connect();

    return () => {
      unmounted = true;
      window.clearTimeout(retryTimer);
      socket?.close();
    };
  }, [applyMessage, setConnection]);
}
