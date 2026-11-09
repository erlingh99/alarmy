import type { IncomingMessage, ServerResponse } from "node:http";
import { randomUUID } from "node:crypto";
import type { Plugin } from "vite";
import type { Alarm } from "./src/types/alarm.ts";

// In-memory stand-in for the FastAPI routes the frontend calls; resets on dev server restart.

type ToggleEvent = { id: number; enabled: boolean; changed_at: string };
type LogEntry = { id: number; message: string; timestamp: string };

const now = () => new Date().toISOString();
const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

function seedAlarms(): Map<string, Alarm> {
  const alarms: Alarm[] = [
    {
      id: randomUUID(),
      name: "Weekdays",
      time: "06:45:00",
      color: "#FFB347",
      length: 30,
      intensity_curve: { start_intensity: 0, end_intensity: 100, curve: "s-curve", hyper_parameter: 5 },
      is_active: true,
      recurrence: { recurrence_type: "weekly", days: [true, true, true, true, true, false, false] },
      play_melody: true,
      created_at: minutesAgo(10_000),
      updated_at: minutesAgo(10_000),
    },
    {
      id: randomUUID(),
      name: "Weekend",
      time: "09:00:00",
      color: "#86A789",
      length: 45,
      intensity_curve: { start_intensity: 0, end_intensity: 80, curve: "linear" },
      is_active: false,
      recurrence: { recurrence_type: "weekly", days: [false, false, false, false, false, true, true] },
      play_melody: false,
      created_at: minutesAgo(5_000),
      updated_at: minutesAgo(5_000),
    },
  ];
  return new Map(alarms.map((a) => [a.id, a]));
}

function seedLogs(): LogEntry[] {
  const lines = [
    "\x1b[32mINFO \x1b[0m firmware: boot complete, version 1.4.0",
    "\x1b[32mINFO \x1b[0m wifi: connected",
    "\x1b[32mINFO \x1b[0m time_lib: synced clock",
    "\x1b[33mWARN \x1b[0m display: init failed, retrying",
    "\x1b[32mINFO \x1b[0m alarm: loaded 1 active alarm",
  ];
  return lines.map((message, i) => ({ id: i + 1, message, timestamp: minutesAgo(lines.length - i) })).reverse();
}

const state = {
  alarms: seedAlarms(),
  toggles: [{ id: 1, enabled: true, changed_at: minutesAgo(10_000) }] as ToggleEvent[],
  logs: seedLogs(),
  firmware: { major: 1, minor: 4, patch: 0 },
};

const alarmsEnabled = () => state.toggles[0]?.enabled ?? true;

async function readJson(req: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  const body = Buffer.concat(chunks).toString();
  return body ? JSON.parse(body) : undefined;
}

function send(res: ServerResponse, status: number, body?: unknown) {
  res.statusCode = status;
  if (body === undefined) {
    res.end();
    return;
  }
  res.setHeader("Content-Type", "application/json");
  res.end(JSON.stringify(body));
}

async function handle(method: string, path: string, query: URLSearchParams, req: IncomingMessage, res: ServerResponse) {
  const alarmId = path.match(/^\/alarms\/([^/]+)$/)?.[1];

  if (path === "/alarms" && method === "GET") return send(res, 200, [...state.alarms.values()]);
  if (path === "/alarms" && method === "PUT") {
    const body = (await readJson(req)) as Omit<Alarm, "id" | "created_at" | "updated_at">;
    const alarm: Alarm = { ...body, id: randomUUID(), created_at: now(), updated_at: now() };
    state.alarms.set(alarm.id, alarm);
    return send(res, 200, alarm);
  }
  if (alarmId) {
    const alarm = state.alarms.get(alarmId);
    if (method === "DELETE") {
      state.alarms.delete(alarmId);
      return send(res, 204);
    }
    if (!alarm) return send(res, 404, { detail: "Alarm not found" });
    if (method === "GET") return send(res, 200, alarm);
    if (method === "PATCH") {
      const updated = { ...alarm, ...((await readJson(req)) as Partial<Alarm>), id: alarm.id, updated_at: now() };
      state.alarms.set(alarmId, updated);
      return send(res, 200, updated);
    }
  }

  if (path === "/mode/animation" && method === "POST") {
    const { type, value } = (await readJson(req)) as { type: string; value?: string };
    if (type === "color" && value == null) return send(res, 422, { detail: "value is required when type is 'color'" });
    return send(res, 200, { message: "Command sent successfully" });
  }
  if (path === "/mode/alarms" && method === "GET") return send(res, 200, { enabled: alarmsEnabled() });
  if (path === "/mode/alarms" && method === "PUT") {
    const { enabled } = (await readJson(req)) as { enabled: boolean };
    if (enabled !== alarmsEnabled()) {
      state.toggles.unshift({ id: state.toggles.length + 1, enabled, changed_at: now() });
    }
    return send(res, 200, { enabled });
  }
  if (path === "/mode/alarms/history" && method === "GET") {
    return send(res, 200, state.toggles.slice(0, Number(query.get("limit") ?? 50)));
  }

  if (path === "/logs" && method === "GET") return send(res, 200, state.logs.slice(0, Number(query.get("max") ?? 100)));

  if (path === "/firmware/version" && method === "GET") return send(res, 200, state.firmware);
  if (path === "/firmware/device-version" && method === "GET") return send(res, 200, state.firmware);

  return send(res, 404, { detail: "Not Found" });
}

export function mockBackend(): Plugin {
  return {
    name: "alarmy-mock-backend",
    apply: "serve",
    configureServer(server) {
      server.config.logger.info("  Using mocked backend for /api (set BACKEND_URL to proxy to a real one)");
      server.middlewares.use("/api", (req, res) => {
        const url = new URL(req.url ?? "/", "http://mock");
        handle(req.method ?? "GET", url.pathname, url.searchParams, req, res).catch((err: unknown) =>
          send(res, 500, { detail: String(err) }),
        );
      });
    },
  };
}
