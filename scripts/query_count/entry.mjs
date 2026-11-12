// Query-count entry for local measurement only (scripts/query_count/run.sh
// copies it next to the built worker). It counts what each request costs on
// Cloudflare's meters: D1 statements, the serial rounds they form (a round
// starts when no other statement is in flight), D1 rows read and written,
// Durable Object requests, Queue messages, KV reads and writes, and Workflow
// instances. The tally is also snapshotted when the response is returned;
// GET /__qc returns both and resets them.
import Base from "./index.js";
export { ContainerStartupOptions, MinifyConfig, R2Range, RateLimitCounter, StreamHub } from "./index.js";

const emptyTally = () => ({
  statements: 0,
  rounds: 0,
  batches: 0,
  sql: [],
  rowsRead: 0,
  rowsWritten: 0,
  rowsBySql: [],
  doRequests: 0,
  queueMessages: 0,
  kvReads: 0,
  kvWrites: 0,
  workflowCreates: 0,
});
let tally = emptyTally();
let atResponse;
let inFlight = 0;

const sqlText = (statement) =>
  String(statement?.statement ?? "?").replace(/\s+/g, " ").slice(0, 160);

const countMeta = (result, sql) => {
  const meta = result?.meta;
  if (!meta) return;
  tally.rowsRead += meta.rows_read ?? 0;
  tally.rowsWritten += meta.rows_written ?? 0;
  tally.rowsBySql.push(`${meta.rows_read ?? 0} ${meta.rows_written ?? 0} ${sql}`);
};

const track = async (count, sql, work) => {
  if (inFlight === 0) tally.rounds += 1;
  inFlight += 1;
  tally.statements += count;
  tally.sql.push(...sql);
  try {
    return await work();
  } finally {
    inFlight -= 1;
  }
};

const marked = (proto) => {
  if (!proto || proto.__queryCountPatched) return false;
  proto.__queryCountPatched = true;
  return true;
};

const patchD1 = (db) => {
  const dbProto = Object.getPrototypeOf(db);
  if (!marked(dbProto)) return;
  const stmtProto = Object.getPrototypeOf(db.prepare("SELECT 1"));
  const all = stmtProto.all;
  for (const method of ["all", "run"]) {
    const original = stmtProto[method];
    stmtProto[method] = function (...args) {
      const sql = sqlText(this);
      return track(1, [sql], async () => {
        const result = await original.apply(this, args);
        countMeta(result, sql);
        return result;
      });
    };
  }
  // `first` hides `meta`; read through `all` so its rows are counted.
  stmtProto.first = function (column) {
    const sql = sqlText(this);
    return track(1, [sql], async () => {
      const result = await all.call(this);
      countMeta(result, sql);
      const row = result?.results?.[0];
      if (row === undefined) return null;
      return typeof column === "string" ? row[column] : row;
    });
  };
  const batch = dbProto.batch;
  dbProto.batch = function (statements) {
    tally.batches += 1;
    const sql = statements.map((statement) => `[batch] ${sqlText(statement)}`);
    return track(statements.length, sql, async () => {
      const results = await batch.call(this, statements);
      results.forEach((result, index) => countMeta(result, sql[index]));
      return results;
    });
  };
};

const counting = (target, onCall) =>
  new Proxy(target, {
    get(object, key) {
      const value = Reflect.get(object, key);
      if (typeof value !== "function" || key === "then" || typeof key === "symbol") return value;
      return (...args) => {
        onCall(key, args);
        return Reflect.apply(value, object, args);
      };
    },
  });

const patchBindings = (env) => {
  for (const value of Object.values(env)) {
    if (typeof value !== "object" || value === null) continue;
    const proto = Object.getPrototypeOf(value);
    if (typeof value.idFromName === "function" && typeof value.get === "function") {
      if (!marked(proto)) continue;
      for (const method of ["get", "getByName"]) {
        const original = proto[method];
        if (typeof original !== "function") continue;
        proto[method] = function (...args) {
          return counting(original.apply(this, args), () => {
            tally.doRequests += 1;
          });
        };
      }
    } else if (typeof value.send === "function" && typeof value.sendBatch === "function") {
      if (!marked(proto)) continue;
      const send = proto.send;
      const sendBatch = proto.sendBatch;
      proto.send = function (...args) {
        tally.queueMessages += 1;
        return send.apply(this, args);
      };
      proto.sendBatch = function (messages, ...rest) {
        tally.queueMessages += Array.isArray(messages) ? messages.length : 1;
        return sendBatch.call(this, messages, ...rest);
      };
    } else if (typeof value.getWithMetadata === "function") {
      if (!marked(proto)) continue;
      for (const method of ["get", "getWithMetadata", "list", "put", "delete"]) {
        const original = proto[method];
        if (typeof original !== "function") continue;
        const write = method === "put" || method === "delete";
        proto[method] = function (...args) {
          if (write) tally.kvWrites += 1;
          else tally.kvReads += 1;
          return original.apply(this, args);
        };
      }
    } else if (typeof value.create === "function" && typeof value.createBatch === "function") {
      if (!marked(proto)) continue;
      const create = proto.create;
      proto.create = function (...args) {
        tally.workflowCreates += 1;
        return create.apply(this, args);
      };
    }
  }
};

let patched = false;

export default class extends Base {
  async fetch(request) {
    if (!patched) {
      patched = true;
      patchD1(this.env.DB);
      patchBindings(this.env);
    }
    const url = new URL(request.url);
    if (url.pathname === "/__qc") {
      const out = { ...tally, atResponse };
      tally = emptyTally();
      atResponse = undefined;
      return Response.json(out);
    }
    const response = await super.fetch(request);
    atResponse = { ...tally, sql: [...tally.sql] };
    return response;
  }
}
