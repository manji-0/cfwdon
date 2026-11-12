// Same scenario as tstodon's query-count.bench.test.ts, over HTTP against a
// worker whose entry exposes /__qc (statements, rounds, batches, sql).
const base = process.env.BASE ?? "http://localhost:8787";
const verbose = process.env.VERBOSE ?? "";
const names = ["alice", "bob", "carol", "f1", "f2", "f3", "f4", "f5", "f6"];
const tok = Object.fromEntries(names.map((n) => [n, `qctoken-${n}`]));
const ids = Object.fromEntries(names.map((n, i) => [n, String(100000000000000000n + BigInt(i))]));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const call = async (who, method, path, body) => {
  const r = await fetch(base + path, {
    method,
    headers: { Authorization: `Bearer ${tok[who]}`, "CF-Connecting-IP": "203.0.113.7", ...(body ? { "Content-Type": "application/json" } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  let json; try { json = JSON.parse(text); } catch { json = text; }
  if (r.status >= 400) console.error("ERR", method, path, r.status, String(text).slice(0, 200));
  return { status: r.status, json };
};
const qc = async () => (await fetch(base + "/__qc")).json();
const stable = (v) => JSON.stringify(v, null, 1).replace(/\d{4}-\d\d-\d\dT[\d:.]+Z?/g, "TS").replace(/\d{15,}/g, "N").replace(/[0-9a-f]{32}/g, "H");
const shape = (label, v) => { if (process.env.SHAPE) console.log("SHAPE|" + label + "|" + stable(v)); };
const id = (r) => { if (!r.json?.id) throw new Error(JSON.stringify(r.json).slice(0, 300)); return r.json.id; };
const measure = async (label, who, method, path, body) => {
  await sleep(400); await qc();
  const r = await call(who, method, path, body);
  const total = await qc();
  const before = total.atResponse ?? total;
  await sleep(600);
  const later = await qc();
  const after = { statements: total.statements - before.statements + later.statements };
  const sum = (key) => total[key] + later[key];
  const size = Array.isArray(r.json) ? r.json.length : "";
  // Rows and binding calls are totals, deferred work included: all of it is billed.
  console.log(`QUERY_COUNT_ROW|${label}|${r.status}|${size}|${before.statements}|${before.rounds}|${before.batches}|${after.statements}|${sum("rowsRead")}|${sum("rowsWritten")}|${sum("doRequests")}|${sum("queueMessages")}|${sum("kvReads")}|${sum("kvWrites")}|${sum("workflowCreates")}`);
  if (verbose && label.includes(verbose)) {
    console.log((before.sql ?? []).map((s) => "  " + String(s).replace(/\s+/g, " ").slice(0, 160)).join("\n"));
    console.log([...total.rowsBySql, ...later.rowsBySql].sort((a, b) => Number(b.split(" ")[0]) - Number(a.split(" ")[0])).slice(0, 8).map((line) => "QUERY_ROWS " + line).join("\n"));
  }
  return r.json;
};
// warm up: migrates legacy plaintext tokens to hashes
for (const n of names) await call(n, "GET", "/api/v1/accounts/verify_credentials");
await call("alice", "POST", `/api/v1/accounts/${ids.bob}/follow`);
await call("alice", "POST", `/api/v1/accounts/${ids.carol}/follow`);
for (const f of ["f1", "f2", "f3", "f4", "f5", "f6"]) await call(f, "POST", `/api/v1/accounts/${ids.alice}/follow`);
const bobStatuses = [];
for (let n = 0; n < 15; n++) {
  const text = n % 3 === 0 ? `post ${n} #qctag @qcalice` : `post ${n}`;
  bobStatuses.push(id(await call("bob", "POST", "/api/v1/statuses", { status: text })));
}
for (const s of bobStatuses.slice(0, 3)) await call("carol", "POST", `/api/v1/statuses/${s}/reblog`);
for (const s of bobStatuses.slice(0, 5)) {
  await call("carol", "POST", `/api/v1/statuses/${s}/favourite`);
  await call("alice", "POST", `/api/v1/statuses/${s}/favourite`);
  await call("alice", "POST", `/api/v1/statuses/${s}/bookmark`);
}
const root = id(await call("alice", "POST", "/api/v1/statuses", { status: "thread root" }));
await call("bob", "POST", `/api/v1/statuses/${root}/favourite`);
const replyIds = [];
let parent = root;
for (let n = 0; n < 10; n++) replyIds.push(parent = id(await call(n % 2 === 0 ? "bob" : "carol", "POST", "/api/v1/statuses", { status: `reply ${n}`, in_reply_to_id: parent })));
for (let n = 0; n < 3; n++) await call("bob", "POST", "/api/v1/statuses", { status: `@qcalice dm ${n}`, visibility: "direct" });
const list = id(await call("alice", "POST", "/api/v1/lists", { title: "qc" }));
await call("alice", "POST", `/api/v1/lists/${list}/accounts`, { account_ids: [ids.bob, ids.carol] });
await sleep(1500);

const target = bobStatuses[14];
await measure("verify_credentials", "alice", "GET", "/api/v1/accounts/verify_credentials");
shape("home", await measure("home(20)", "alice", "GET", "/api/v1/timelines/home?limit=20"));
await measure("public local(20)", "alice", "GET", "/api/v1/timelines/public?local=true&limit=20");
await measure("public(20)", "alice", "GET", "/api/v1/timelines/public?limit=20");
await measure("tag(20)", "alice", "GET", "/api/v1/timelines/tag/qctag?limit=20");
await measure("list(20)", "alice", "GET", `/api/v1/timelines/list/${list}?limit=20`);
shape("status", await measure("status", "alice", "GET", `/api/v1/statuses/${target}`));
shape("carolboost", (await call("alice", "GET", "/api/v1/accounts/" + ids.carol + "/statuses?limit=5")).json);
const ctx = await measure("context(10 replies)", "alice", "GET", `/api/v1/statuses/${root}/context`);
console.log("CONTEXT_SHAPE|" + JSON.stringify({ a: ctx.ancestors.map((s) => s.content), d: ctx.descendants.map((s) => [s.content, s.account.username, s.replies_count, s.favourites_count]) }));
const midCtx = await call("alice", "GET", `/api/v1/statuses/${replyIds[4]}/context`);
console.log("CONTEXT_SHAPE_MID|" + JSON.stringify({ a: midCtx.json.ancestors.map((s) => s.content), d: midCtx.json.descendants.map((s) => s.content) }));
await measure("account", "alice", "GET", `/api/v1/accounts/${ids.bob}`);
await measure("account statuses(20)", "alice", "GET", `/api/v1/accounts/${ids.bob}/statuses?limit=20`);
await measure("relationships(2)", "alice", "GET", `/api/v1/accounts/relationships?id[]=${ids.bob}&id[]=${ids.carol}`);
await measure("notifications(20)", "alice", "GET", "/api/v1/notifications?limit=20");
await measure("bookmarks(20)", "alice", "GET", "/api/v1/bookmarks?limit=20");
await measure("favourites(20)", "alice", "GET", "/api/v1/favourites?limit=20");
await measure("conversations", "alice", "GET", "/api/v1/conversations");
await measure("POST status(tag+mention)", "alice", "POST", "/api/v1/statuses", { status: "hello #qctag @qcbob" });
await measure("POST favourite", "alice", "POST", `/api/v1/statuses/${target}/favourite`);
await measure("POST unfavourite", "alice", "POST", `/api/v1/statuses/${target}/unfavourite`);
shape("reblog", await measure("POST reblog", "alice", "POST", `/api/v1/statuses/${target}/reblog`));
shape("home_after", (await call("f1", "GET", "/api/v1/timelines/home?limit=5")).json);
await measure("POST unreblog", "alice", "POST", `/api/v1/statuses/${target}/unreblog`);
await measure("POST bookmark", "alice", "POST", `/api/v1/statuses/${target}/bookmark`);
await measure("POST follow", "alice", "POST", `/api/v1/accounts/${ids.f1}/follow`);
await measure("POST unfollow", "alice", "POST", `/api/v1/accounts/${ids.f1}/unfollow`);
