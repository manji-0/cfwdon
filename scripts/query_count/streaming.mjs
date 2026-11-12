// Streaming fan-out against the registry: with subscribers connected, a post
// reaches each of them and costs one DO request per subscribed hub; once they
// disconnect, a post costs only the rate limiter's DO request.
// Run with `SCENARIO=streaming bash scripts/query_count/run.sh`.
import assert from "node:assert/strict";

const base = process.env.BASE ?? "http://localhost:8787";
const names = ["alice", "bob", "carol", "f1", "f2"];
const tok = Object.fromEntries(names.map((n) => [n, `qctoken-${n}`]));
const ids = Object.fromEntries(names.map((n, i) => [n, String(100000000000000000n + BigInt(i))]));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// Longer than the publishers' registry cache, so the next post sees the change.
const settle = () => sleep(6000);
const call = async (who, method, path, body) => {
  const r = await fetch(base + path, {
    method,
    headers: { Authorization: `Bearer ${tok[who]}`, "CF-Connecting-IP": "203.0.113.7", ...(body ? { "Content-Type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  assert.ok(r.status < 400, `${method} ${path} ${r.status} ${text.slice(0, 200)}`);
  return JSON.parse(text);
};
const qc = async () => (await fetch(base + "/__qc")).json();

const connect = (who, query) =>
  new Promise((resolve, reject) => {
    const url = `${base.replace(/^http/, "ws")}/api/v1/streaming?${query}&access_token=${tok[who]}`;
    const socket = new WebSocket(url);
    const events = [];
    socket.addEventListener("message", (message) => events.push(JSON.parse(message.data)));
    socket.addEventListener("open", () => resolve({ socket, events }));
    socket.addEventListener("error", () => reject(new Error(`${who} could not connect: ${query}`)));
  });

const post = async (status) => {
  await sleep(400);
  await qc();
  const created = await call("alice", "POST", "/api/v1/statuses", { status });
  await sleep(1500);
  const tally = await qc();
  return { id: created.id, doRequests: tally.doRequests, statements: tally.statements };
};

for (const n of names) await call(n, "GET", "/api/v1/accounts/verify_credentials");
for (const f of ["f1", "f2"]) await call(f, "POST", `/api/v1/accounts/${ids.alice}/follow`);

const idle = await post("before anyone listens #qctag @qcbob");
console.log(`STREAM_E2E|no subscribers|do_requests=${idle.doRequests}|statements=${idle.statements}`);
assert.equal(idle.doRequests, 1, "only the rate limiter is called with nobody listening");

const home = await connect("f1", "stream=user");
const mentions = await connect("bob", "stream=user:notification");
const tag = await connect("carol", "stream=hashtag&tag=qctag");
await settle();

const live = await post("while they listen #qctag @qcbob");
console.log(`STREAM_E2E|3 subscribers|do_requests=${live.doRequests}|statements=${live.statements}`);
const received = (client, event) =>
  client.events.some((message) => message.event === event && String(message.payload).includes(live.id));
assert.ok(received(home, "update"), "f1's home stream receives the post");
assert.ok(received(mentions, "notification"), "bob's notification stream receives the mention");
assert.ok(received(tag, "update"), "carol's hashtag stream receives the post");
console.log("STREAM_E2E|delivered|f1 user update, bob mention notification, carol hashtag update");

for (const client of [home, mentions, tag]) client.socket.close();
await settle();

const after = await post("after they left #qctag @qcbob");
console.log(`STREAM_E2E|subscribers gone|do_requests=${after.doRequests}|statements=${after.statements}`);
assert.equal(after.doRequests, 1, "closed sockets leave no registry rows behind");
