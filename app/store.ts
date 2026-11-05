// The app's state and what changes it: the open files, the current request (the one the caret is in), the
// environment, runs and their streamed events. Files save themselves half a second after an edit and reload when
// changed outside (an agent editing them), unless the editor has unsaved changes.
import { cleanup, computed, effect, signal, untracked } from "nsh";
import type { EditorRef } from "nsh/base";
import { Callme } from "callme/native";
import { api, apiError, project, type ApiError, type EnvView, type RequestRef, type Run } from "./api";

export type RunState = { running: boolean; run?: Run; events: unknown[]; status?: string; error?: ApiError };

export const requests = signal<RequestRef[]>(project() ? api.list() : []);
export const files = signal<string[]>(project() ? api.files() : []);
export const tabs = signal<string[]>([]);
export const activeFile = signal("");
export const text = signal("");
export const cursorLine = signal(0);
export const view = signal<"request" | "env">("request");
export const envTab = signal<"public" | "private">("public");
export const envState = signal<EnvView>(safeEnvView());
export const paletteOpen = signal(false);
export const runs = signal<Record<string, RunState>>({});
export const notice = signal<string | null>(null);

export const editor: { ref?: EditorRef; response?: EditorRef; env?: EditorRef } = {};

const saved = new Map<string, { text: string; mtime: number }>();

function safeEnvView(): EnvView {
  if (!project()) return { envs: [], vars: [], current: null, env: null };
  try {
    return api.envView();
  } catch (e) {
    return { envs: [], vars: [], current: null, env: null };
  }
}

export const env = computed(() => envState().current ?? null);

/** `source`'s value once it has stayed the same for `ms` (work that is too slow for every keystroke). */
export function settled<T>(source: () => T, ms = 250) {
  const value = signal(untracked(source));
  let timer: ReturnType<typeof setTimeout> | undefined;
  effect(() => {
    const next = source();
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => value.set(next), ms);
  });
  return value;
}

/** The text once typing paused. */
export const settledText = settled(() => text());

/** The 1-based index of the request the caret is in. */
export const currentIndex = computed(() => (text() ? api.requestAt(text(), cursorLine()) : 0));

/** How the current request is addressed: its name, else `#index`. */
export const currentRequest = computed<RequestRef | null>(() => {
  const i = currentIndex();
  if (!i) return null;
  return requests().find((r) => r.file === activeFile() && r.index === i) ?? null;
});

export const runKey = (file: string, id: string) => `${file}#${id}`;

/** The current request's run: this session's, else its newest history record. */
export const currentRun = computed<RunState | null>(() => {
  const r = currentRequest();
  if (!r) return null;
  const live = runs()[runKey(r.file, r.id)];
  if (live) return live;
  const [last] = api.history(r.file, r.id, 1);
  return last ? { running: false, run: last, events: [] } : null;
});

export function refresh() {
  requests.set(project() ? api.list() : []);
  files.set(project() ? api.files() : []);
}

/** Opens the project in folder `path`: its requests, its environments, its first request. */
export function openProject(path: string) {
  saveNow();
  project.set(path);
  Callme.remember(path);
  saved.clear();
  tabs.set([]);
  activeFile.set("");
  text.set("");
  cursorLine.set(0);
  runs.set({});
  recent.set([]);
  view.set("request");
  refresh();
  reloadEnv();
  const first = requests()[0];
  if (first) selectRequest(first);
}

/** Which name is being typed in the sidebar: a new file, or a new name for a file. */
export const naming = signal<{ kind: "create" } | { kind: "rename"; file: string } | { kind: "request"; file: string; index: number } | null>(null);

/** Whether the environment view is asking for a new environment's name. */
export const creatingEnv = signal(false);

export function createFile(name: string) {
  try {
    const file = api.create(name);
    naming.set(null);
    refresh();
    openFile(file);
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

export function renameFile(from: string, to: string) {
  try {
    saveNow();
    const file = api.rename(from, to);
    naming.set(null);
    saved.delete(from);
    tabs.set(tabs().map((f) => (f === from ? file : f)));
    if (activeFile() === from) {
      activeFile.set(file);
      saved.set(file, { text: text(), mtime: api.modified(file) });
    }
    refresh();
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

export async function trashFile(file: string) {
  try {
    if (activeFile() === file) saveNow();
    await api.trash(file);
    if (tabs().includes(file)) closeTab(file);
    saved.delete(file);
    refresh();
    notice.set(`已把 ${file} 移到废纸篓`);
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

/** Names request `index` of `file`, the references to its old name following. */
export function renameRequest(file: string, index: number, name: string) {
  try {
    if (file !== activeFile()) openFile(file);
    const next = api.renameRequest(file, text(), index, name);
    naming.set(null);
    text.set(next);
    saveNow();
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

/** Adds environment `name` with the current environment's variable names, and switches to it. */
export function addEnv(name: string): boolean {
  try {
    api.addEnv(name, env());
    creatingEnv.set(false);
    reloadEnv();
    notice.set(`已新建环境 ${name.trim()}，在 http-client.env.json 里填上变量的值`);
    return true;
  } catch (e) {
    notice.set(apiError(e).message);
    return false;
  }
}

/** Asks for a folder and opens it as the project. */
export async function pickProject() {
  try {
    const path = await Callme.pickFolder();
    if (path) openProject(path);
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

export function openFile(file: string, line?: number) {
  saveNow();
  if (!tabs().includes(file)) tabs.set([...tabs(), file]);
  if (file !== activeFile()) {
    const t = api.read(file);
    saved.set(file, { text: t, mtime: api.modified(file) });
    activeFile.set(file);
    text.set(t);
    settledText.set(t);
    cursorLine.set(0);
  }
  view.set("request");
  if (line !== undefined) {
    cursorLine.set(line);
    setTimeout(() => editor.ref?.goToLine(line), 0);
  }
}

export function closeTab(file: string) {
  saveNow();
  const rest = tabs().filter((f) => f !== file);
  tabs.set(rest);
  if (file === activeFile()) {
    activeFile.set("");
    text.set("");
    if (rest.length) openFile(rest[rest.length - 1]);
  }
}

/** Opens request `r` with the caret on its request line. */
export function selectRequest(r: RequestRef) {
  openFile(r.file);
  const line = api.requestLine(text(), r.index);
  cursorLine.set(line);
  setTimeout(() => editor.ref?.goToLine(line), 0);
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;

export function setText(value: string) {
  text.set(value);
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(saveNow, 500);
}

export function saveNow() {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = undefined;
  const file = activeFile();
  if (!file) return;
  const last = saved.get(file);
  if (last && last.text === text()) return;
  api.write(file, text());
  saved.set(file, { text: text(), mtime: api.modified(file) });
  refresh();
}

/** Reloads the active file when it changed on disk; keeps the editor's text when it has unsaved edits. */
export function checkExternalChanges() {
  const file = activeFile();
  if (!file) return;
  const last = saved.get(file);
  const mtime = api.modified(file);
  if (!last || mtime <= last.mtime) return;
  const disk = api.read(file);
  if (disk === last.text) {
    saved.set(file, { text: disk, mtime });
    return;
  }
  if (text() === last.text) {
    saved.set(file, { text: disk, mtime });
    text.set(disk);
    refresh();
    notice.set(`${file} 已被外部修改，已重新加载`);
  } else {
    saved.set(file, { text: last.text, mtime });
    notice.set(`${file} 已被外部修改，但这里有未保存的改动：保存会覆盖外部的修改`);
  }
}

export function setEnv(name: string) {
  api.setEnv(name);
  envState.set(safeEnvView());
}

export function reloadEnv() {
  envState.set(safeEnvView());
}

function patchRun(key: string, patch: Partial<RunState>) {
  const all = runs();
  runs.set({ ...all, [key]: { ...(all[key] ?? { running: false, events: [] }), ...patch } });
}

/** Sends the current request (saving the file first). */
export async function send() {
  saveNow();
  const file = activeFile();
  const index = currentIndex();
  if (!file || !index) return;
  const r = requests().find((x) => x.file === file && x.index === index);
  const id = r?.id ?? `#${index}`;
  const key = runKey(file, id);
  if (runs()[key]?.running) return;
  patchRun(key, { running: true, events: [], status: "发送中…", error: undefined, run: undefined });
  try {
    const run = await api.run(file, id, env(), (m) => {
      if (m.type === "head" && m.streaming) patchRun(key, { status: `${m.status} · 流式接收中…` });
      if (m.type === "event") patchRun(key, { events: [...(runs()[key]?.events ?? []), m.event] });
    });
    patchRun(key, { running: false, run, status: undefined });
    recent.set([{ file, id, method: run.request.method, status: run.response.status, at: run.at }, ...recent().filter((x) => runKey(x.file, x.id) !== key)].slice(0, 6));
  } catch (e) {
    patchRun(key, { running: false, status: undefined, error: apiError(e) });
  }
}

export const recent = signal<{ file: string; id: string; method: string; status: number; at: string }[]>([]);

/** Adds a request at the end of the active file (a new file when none is open) and puts the caret on it. */
export function newRequest() {
  if (!project()) return;
  if (!activeFile()) {
    naming.set({ kind: "create" });
    return;
  }
  const body = text().replace(/\s*$/, "");
  const names = new Set(requests().filter((r) => r.file === activeFile()).map((r) => r.name));
  let name = "request";
  for (let i = 2; names.has(name); i++) name = `request${i}`;
  const block = `### 新请求\n# @name ${name}\nGET {{baseUrl}}/\nAccept: application/json\n`;
  const next = body ? `${body}\n\n${block}` : block;
  setText(next);
  saveNow();
  const line = next.split("\n").length - 3;
  cursorLine.set(line);
  const added = requests().filter((r) => r.file === activeFile()).length;
  // Name it first; the caret goes to the end of its URL once the name is set (`afterNaming`).
  naming.set({ kind: "request", file: activeFile(), index: added });
}

/** After a request was named: the caret at the end of its URL, the editor focused. */
export function afterNaming(index: number) {
  setTimeout(() => {
    const line = api.requestLine(text(), index);
    const col = text().split("\n")[line]?.length ?? 0;
    cursorLine.set(line);
    editor.ref?.setSelections([{ anchor: { line, column: col }, head: { line, column: col } }]);
    editor.ref?.focus();
  }, 0);
}

/** Goes to a definition: a line of an environment file in the environment view, else a line of a `.http` file. */
export function goTo(file: string, line: number) {
  if (file === "http-client.env.json" || file === "http-client.private.env.json") {
    saveNow();
    envTab.set(file === "http-client.env.json" ? "public" : "private");
    view.set("env");
    setTimeout(() => editor.env?.goToLine(line), 50);
    return;
  }
  openFile(file, line);
}

/** The request a run failed for because it had not run yet (`请求 \`login\` 还没有运行过`), else null. */
export function missingDependency(message: string | undefined): string | null {
  const m = message?.match(/请求 `([^`]+)` 还没有运行过/);
  return m ? m[1] : null;
}

/** Runs request `name` (the active file's first, else the project's), then the current request again. */
export async function sendAfter(name: string) {
  const dep = requests().find((r) => r.name === name && r.file === activeFile()) ?? requests().find((r) => r.name === name);
  if (!dep) return;
  const key = runKey(dep.file, dep.id);
  patchRun(key, { running: true, events: [], error: undefined });
  try {
    const run = await api.run(dep.file, dep.id, env(), () => {});
    patchRun(key, { running: false, run });
  } catch (e) {
    patchRun(key, { running: false, error: apiError(e) });
    notice.set(`${name} 运行失败：${apiError(e).message}`);
    return;
  }
  await send();
}

/** Pretty-prints the current request's JSON body. */
export function formatBody() {
  const index = currentIndex();
  if (!activeFile() || !index) return;
  try {
    const next = api.formatBody(text(), index);
    if (next === null) return notice.set("当前请求没有可以格式化的请求体");
    if (next !== text()) setText(next);
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

/** Copies the current request as a curl command. */
export function copyCurl() {
  const index = currentIndex();
  if (!activeFile() || !index) return;
  try {
    api.copy(api.curl(activeFile(), text(), index, env()));
    notice.set("已复制为 curl");
  } catch (e) {
    notice.set(apiError(e).message);
  }
}

// Notices go after a few seconds.
effect(() => {
  if (notice()) {
    const t = setTimeout(() => notice.set(null), 4000);
    cleanup(() => clearTimeout(t));
  }
});
