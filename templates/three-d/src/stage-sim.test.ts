import { describe, expect, it } from "vitest";
import type { JobPhase, JobState, NodeState, PipelineData, Snapshot } from "@pipeline-viz/protocol";
import { applySnapshot, STALL_THRESHOLD_MS } from "@pipeline-viz/protocol";
import { orderNodeIds, StageSim } from "./stage-sim";

function node(node_id: string, kind: NodeState["kind"], inputs: string[]): NodeState {
  return {
    node_id,
    display_name: node_id,
    kind,
    inputs,
    counters: { in_flight: 1, queue_depth: 0, left_total: 0, throughput_per_sec: 0, p50_ms: 0, p95_ms: 0 },
  };
}

function job(
  job_id: string,
  current_node: string,
  phase: JobPhase,
  entered_node_at_ms: number,
): JobState {
  return {
    job_id,
    job_type: "block",
    current_node,
    ...phase,
    entered_node_at_ms,
    created_at_ms: entered_node_at_ms - 100,
    meta: {},
  };
}

function data(jobs: JobState[], nodes?: NodeState[], ts = 10_000): PipelineData {
  const snapshot: Snapshot = {
    type: "snapshot",
    ts_ms: ts,
    nodes: nodes ?? [node("fetcher", "source", []), node("indexer", "transform", ["fetcher"])],
    jobs,
    dropped_events: 0,
    process: null,
  };
  return applySnapshot(snapshot);
}

describe("StageSim ingest — snapshot", () => {
  it("maps nodes to stage shape and lays them out by graph rank", () => {
    const sim = new StageSim();
    const graphNodes = [
      node("fetcher", "source", []),
      node("indexer", "transform", ["fetcher"]),
      node("committer", "sink", ["indexer"]),
    ];
    sim.ingest(data([], graphNodes));

    expect(Object.keys(sim.nodes)).toEqual(["fetcher", "indexer", "committer"]);
    // One column per graph rank: fetcher 0, indexer 1, committer 2.
    expect(sim.positions["fetcher"].x).toBeLessThan(sim.positions["indexer"].x);
    expect(sim.positions["indexer"].x).toBeLessThan(sim.positions["committer"].x);
    expect(sim.positions["fetcher"].z).toBe(sim.positions["indexer"].z);
    expect(sim.nodes["indexer"].inputs).toEqual(["fetcher"]);
    expect(sim.nodes["indexer"].display_name).toBe("indexer");
    expect(sim.nodes["indexer"].kind).toBe("transform");
    expect(orderNodeIds(graphNodes)).toEqual(["fetcher", "indexer", "committer"]);
  });

  it("maps wire phases to stage phases", () => {
    const sim = new StageSim();
    sim.ingest(
      data([
        job("a", "fetcher", { phase: "active" }, 100),
        job("b", "indexer", { phase: "held", reason: "waiting for finality" }, 200),
        job("c", "committer", { phase: "abandoned" }, 300),
      ]),
    );

    const byId = new Map(sim.jobList().map((j) => [j.job_id, j]));
    expect(byId.get("a")?.phase).toBe("active");
    expect(byId.get("b")?.phase).toBe("held");
    expect(byId.get("c")?.phase).toBe("abandoned");
  });

  it("ingests a representative thousand-record payload", () => {
    const sim = new StageSim();
    const jobs = Array.from({ length: 1_000 }, (_, index) => ({
      ...job(`job_${index.toString().padStart(4, "0")}`, "indexer", { phase: "held", reason: "r".repeat(80) }, 100),
      meta: { height: index.toString(), source: "representative" },
    }));

    sim.ingest(data(jobs));

    expect(sim.jobList()).toHaveLength(1_000);
    expect(sim.jobList().at(-1)?.job_id).toBe("job_0999");
  });

  it("queues items beyond the reported in-flight capacity, oldest first", () => {
    const sim = new StageSim();
    sim.ingest(
      data([
        job("old", "fetcher", { phase: "active" }, 100),
        job("mid", "fetcher", { phase: "active" }, 200),
        job("new", "fetcher", { phase: "active" }, 300),
      ]),
    );

    const byId = new Map(sim.jobList().map((j) => [j.job_id, j]));
    expect(byId.get("old")?.queued).toBe(false);
    expect(byId.get("mid")?.queued).toBe(true);
    expect(byId.get("new")?.queued).toBe(true);
  });

  it("never queues abandoned items", () => {
    const sim = new StageSim();
    sim.ingest(
      data([
        job("live", "fetcher", { phase: "active" }, 100),
        job("dead", "fetcher", { phase: "abandoned" }, 200),
      ]),
    );

    const dead = sim.jobList().find((j) => j.job_id === "dead");
    expect(dead?.queued).toBe(false);
  });

  it("ages anchored items against the wire clock, clamped at zero", () => {
    const sim = new StageSim();
    sim.ingest(data([job("a", "fetcher", { phase: "held", reason: "x" }, 8_000)]));

    const [stageJob] = sim.jobList();
    expect(sim.ageOf(stageJob, performance.now())).toBe(2_000);
    expect(sim.stallThresholdMs).toBe(STALL_THRESHOLD_MS);
  });

  it("exposes select and fires the onSelect callback", () => {
    const sim = new StageSim();
    const seen: Array<string | null> = [];
    sim.onSelect = (id) => seen.push(id);

    sim.select("fetcher");
    sim.select(null);

    expect(sim.selected).toBeNull();
    expect(seen).toEqual(["fetcher", null]);
  });
});

describe("StageSim ingest — patch deltas", () => {
  const base = (): PipelineData =>
    data([job("a", "fetcher", { phase: "active" }, 100)], undefined, 10_000);

  it("spawns travel when an item changes nodes", () => {
    const sim = new StageSim();
    sim.ingest(base());

    sim.ingest({
      ...base(),
      jobs: { a: job("a", "indexer", { phase: "active" }, 10_100) },
      ts_ms: 10_100,
    });

    const [moved] = sim.jobList();
    expect(moved.mode).toBe("travel");
    expect(moved.travel?.from).toBe("fetcher");
    expect(moved.travel?.to).toBe("indexer");
    expect(moved.travel?.dur).toBeGreaterThan(0);
  });

  it("drops jobs that vanished from the data", () => {
    const sim = new StageSim();
    sim.ingest(base());

    sim.ingest({ ...base(), jobs: {}, ts_ms: 10_100 });

    expect(sim.jobList()).toHaveLength(0);
  });

  it("keeps an in-flight travel gliding across unchanged patches", () => {
    const sim = new StageSim();
    sim.ingest(base());
    sim.ingest({
      ...base(),
      jobs: { a: job("a", "indexer", { phase: "active" }, 10_100) },
      ts_ms: 10_100,
    });
    const travel = sim.jobList()[0]?.travel;

    sim.ingest({
      ...base(),
      jobs: { a: job("a", "indexer", { phase: "active" }, 10_100) },
      ts_ms: 10_200,
    });

    const [still] = sim.jobList();
    expect(still.mode).toBe("travel");
    expect(still.travel).toEqual(travel);
  });

  it("updates node counters from a patch", () => {
    const sim = new StageSim();
    sim.ingest(base());

    sim.ingest({
      ...base(),
      nodes: {
        ...base().nodes,
        fetcher: { ...base().nodes.fetcher!, counters: { ...base().nodes.fetcher!.counters, in_flight: 4 } },
      },
      ts_ms: 10_100,
    });

    expect(sim.nodes["fetcher"].counters.in_flight).toBe(4);
  });
});
