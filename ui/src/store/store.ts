import { create } from "zustand";
import type { ServerMessage } from "../protocol/types";
import { applyPatch, applySnapshot, emptyPipeline, type PipelineData } from "./reduce";

export type ConnectionState = "connecting" | "live" | "reconnecting";

interface PipelineStore {
  data: PipelineData;
  connection: ConnectionState;
  selectedNode: string | null;
  applyMessage: (message: ServerMessage) => void;
  setConnection: (connection: ConnectionState) => void;
  selectNode: (node_id: string | null) => void;
}

export const usePipelineStore = create<PipelineStore>((set) => ({
  data: emptyPipeline(),
  connection: "connecting",
  selectedNode: null,
  applyMessage: (message) =>
    set((state) => ({
      data:
        message.type === "snapshot" ? applySnapshot(message) : applyPatch(state.data, message),
    })),
  setConnection: (connection) => set({ connection }),
  selectNode: (selectedNode) => set({ selectedNode }),
}));
