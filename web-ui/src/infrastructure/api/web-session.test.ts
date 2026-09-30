import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchWebSession } from "@/infrastructure/api/web-session";

const respond = (status: number, body: unknown) =>
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response(JSON.stringify(body), { status })),
  );

describe("fetchWebSession", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("parses the signed-in account", async () => {
    respond(200, {
      id: "1",
      username: "alice",
      display_name: "Alice",
      acct: "alice",
      avatar: "",
      instance_name: "example.test",
    });
    const result = await fetchWebSession();
    expect(result._unsafeUnwrap()?.instanceName).toBe("example.test");
  });

  it("treats 401 as signed out", async () => {
    respond(401, { error: "unauthorized" });
    expect((await fetchWebSession())._unsafeUnwrap()).toBeNull();
  });

  it("surfaces other HTTP errors and invalid payloads", async () => {
    respond(500, "boom");
    expect((await fetchWebSession())._unsafeUnwrapErr()).toMatchObject({
      kind: "HttpStatus",
      status: 500,
    });
    respond(200, { id: "" });
    expect((await fetchWebSession())._unsafeUnwrapErr()).toEqual({ kind: "ValidationError" });
  });
});
