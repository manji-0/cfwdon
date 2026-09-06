import { describe, expect, it } from "vitest";
import { FilterAction, FilterContext } from "@/domain/filters/filter";

describe("FilterContext", () => {
  it("labels known contexts", () => {
    expect(FilterContext.label("home")).toBe("ホーム");
    expect(FilterContext.label("notifications")).toBe("通知");
    expect(FilterContext.label("unknown")).toBe("unknown");
  });

  it("keeps known contexts from the wire", () => {
    expect(FilterContext.fromApiList(["home", "nope", "thread"])).toEqual(["home", "thread"]);
  });

  it("toggles contexts", () => {
    expect(FilterContext.toggle(["home"], "notifications")).toEqual(["home", "notifications"]);
    expect(FilterContext.toggle(["home", "public"], "home")).toEqual(["public"]);
  });
});

describe("FilterAction", () => {
  it("maps unknown wire values to warn", () => {
    expect(FilterAction.fromApi("hide")).toBe("hide");
    expect(FilterAction.fromApi("blur")).toBe("blur");
    expect(FilterAction.fromApi("nope")).toBe("warn");
  });

  it("labels actions", () => {
    expect(FilterAction.label("warn")).toBe("警告");
    expect(FilterAction.label("hide")).toBe("隠す");
    expect(FilterAction.label("blur")).toBe("ぼかす");
  });
});
