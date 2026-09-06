import { describe, expect, it } from "vitest";
import { FilterAction, FilterContext, FilterExpire } from "@/domain/filters/filter";

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

describe("FilterExpire", () => {
  it("omits seconds for never and keep", () => {
    expect(FilterExpire.seconds("never")).toBeUndefined();
    expect(FilterExpire.seconds("keep")).toBeUndefined();
  });

  it("maps duration presets to seconds", () => {
    expect(FilterExpire.seconds("30m")).toBe(1800);
    expect(FilterExpire.seconds("1h")).toBe(3600);
    expect(FilterExpire.seconds("6h")).toBe(21600);
    expect(FilterExpire.seconds("12h")).toBe(43200);
    expect(FilterExpire.seconds("1d")).toBe(86400);
    expect(FilterExpire.seconds("7d")).toBe(604800);
  });

  it("labels create and edit presets", () => {
    expect(FilterExpire.label("never")).toBe("期限なし");
    expect(FilterExpire.label("keep")).toBe("期限を変更しない");
    expect(FilterExpire.label("30m")).toBe("30分");
    expect(FilterExpire.createPresets).not.toContain("keep");
    expect(FilterExpire.editPresets).not.toContain("never");
  });
});
