import { describe, expect, it } from "vitest";
import { newFlowBranch, makeLeaf, addStep, connect } from "./flowGraph";
import { buildFlowYaml } from "./flowYamlSerialize";
import { parseFlowYaml } from "./flowYamlParse";
import type { FlowNode, TextOp } from "./flowModel";

type TextTransformNode = Extract<FlowNode, { kind: "text_transform" }>;

/** A recent pass touched `flowGraph.ts` (the dangerous-action
 *  blacklist) and `App.tsx` extensively — neither should have any
 *  bearing on `text_transform`, but this exists as a direct
 *  regression guard specifically for it: build a flow containing one,
 *  serialize it to `.relay` YAML, parse that back, and check every
 *  field survived unchanged. */
describe("text_transform node YAML round-trip", () => {
  const ops: TextOp[] = ["uppercase", "lowercase", "trim", "replace", "split"];

  it.each(ops)("preserves a %s node through save/load", (op) => {
    let flow = newFlowBranch();
    const node: TextTransformNode = { ...(makeLeaf("text_transform") as TextTransformNode), op, text: "%input%", arg1: "needle", arg2: "replacement", variable: "text_result" };
    flow = addStep(flow, flow.entry, node);
    flow = connect(flow, flow.entry as string, null, node.id);

    const yaml = buildFlowYaml(flow, "text_transform test");
    const { flow: reloaded } = parseFlowYaml(yaml);

    const reloadedNode = reloaded.steps.find((n) => n.kind === "text_transform");
    expect(reloadedNode).toMatchObject({
      kind: "text_transform",
      op,
      text: "%input%",
      arg1: "needle",
      arg2: "replacement",
      variable: "text_result",
      enabled: true,
    });
  });
});
