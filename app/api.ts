// Typed wrappers over `callme/native` (src/lib.rs), which answers in JSON strings.
import { signal } from "nsh";
import { Callme } from "callme/native";

export type RequestRef = { file: string; id: string; name?: string | null; index: number; method: string; url: string; title?: string | null };

export type Headers = Record<string, string | string[]>;

export type Run = {
  at: string;
  request: { file: string; id: string; env?: string | null; method: string; url: string; headers: Headers; addedHeaders?: string[] };
  response: {
    status: number;
    reason: string;
    httpVersion: string;
    headers: Headers;
    bodyKind: "json" | "sse" | "text" | "binary" | "empty";
    body: unknown;
    bodyTruncated: boolean;
    size: number;
    bodyFile?: string;
  };
  timing: { dns: number; connect: number; tls: number; ttfb: number; download: number; total: number };
  historyFile?: string;
};

export type Diagnostic = { line: number; start: number; end: number; severity: "error" | "warning"; message: string };
export type CompletionItem = { label: string; kind: string; detail?: string; insertText?: string };
export type Hover = {
  name: string;
  line: number;
  start: number;
  end: number;
  kind: "variable" | "response" | "dynamic";
  values: { env: string; value?: string | null; source?: string | null; current: boolean }[];
  value?: string | null;
  note?: string | null;
  definedIn?: string | null;
};
export type Definition = { file: string; line: number; start: number; end: number };
export type EnvView = { envs: string[]; current?: string | null; env?: string | null; vars: { name: string; value: string; source: string }[] };
export type Fold = { startLine: number; endLine: number };
export type Span = { line: number; start: number; end: number };

export type ApiError = { kind: string; message: string; hint?: string };

/** The `{ error }` a native call failed with. */
export function apiError(e: unknown): ApiError {
  const text = e instanceof Error ? e.message : String(e);
  try {
    const parsed = JSON.parse(text);
    if (parsed?.error) return parsed.error;
  } catch {}
  return { kind: "internal", message: text };
}

/** The open project's folder; empty while none is open. */
export const project = signal(Callme.root());
const root = () => project();

const opt = (s: string | null | undefined) => (s ? s : null);

export const api = {
  list(): RequestRef[] {
    try {
      return JSON.parse(Callme.list(root()));
    } catch {
      return [];
    }
  },
  files: (): string[] => JSON.parse(Callme.files(root())),
  create: (file: string) => Callme.create(root(), file),
  rename: (from: string, to: string) => Callme.rename(root(), from, to),
  trash: (file: string) => Callme.trash(root(), file),
  reveal: (file: string) => Callme.reveal(root(), file),
  addEnv: (name: string, like?: string | null) => Callme.addEnv(root(), name, like ? like : null),
  read: (file: string) => Callme.read(root(), file),
  write: (file: string, text: string) => Callme.write(root(), file, text),
  modified: (file: string) => Callme.modified(root(), file),
  httpFolds: (text: string): Fold[] => JSON.parse(Callme.httpFolds(text)),
  jsonFolds: (text: string): Fold[] => JSON.parse(Callme.jsonFolds(text)),
  variables: (text: string): Span[] => JSON.parse(Callme.variables(text)),
  requestAt: (text: string, line: number) => Callme.requestAt(text, line),
  renameRequest: (file: string, text: string, index: number, name: string) => Callme.renameRequest(root(), file, text, index, name),
  formatBody: (text: string, index: number): string | null => Callme.formatBody(text, index),
  requestLine: (text: string, index: number) => Callme.requestLine(text, index),
  gitignoreMissing: (): string[] => JSON.parse(Callme.gitignoreMissing(root())),
  addToGitignore: (patterns: string[]) => Callme.addToGitignore(root(), patterns),
  envDiagnostics: (file: string, text: string): Diagnostic[] => JSON.parse(Callme.envDiagnostics(root(), file, text)),
  diagnostics: (file: string, text: string, env?: string | null): Diagnostic[] => JSON.parse(Callme.diagnostics(root(), file, text, opt(env))),
  completions: (file: string, text: string, line: number, column: number, env?: string | null): CompletionItem[] =>
    JSON.parse(Callme.completions(root(), file, text, line, column, opt(env))),
  hover(file: string, text: string, line: number, column: number, env?: string | null): Hover | null {
    const h = Callme.hover(root(), file, text, line, column, opt(env));
    return h ? JSON.parse(h) : null;
  },
  definition(file: string, text: string, line: number, column: number, env?: string | null): Definition | null {
    const d = Callme.definition(root(), file, text, line, column, opt(env));
    return d ? JSON.parse(d) : null;
  },
  preview(file: string, text: string, index: number, env?: string | null): { method: string; url: string } | ApiError {
    try {
      return JSON.parse(Callme.preview(root(), file, text, index, opt(env)));
    } catch (e) {
      return apiError(e);
    }
  },
  curl: (file: string, text: string, index: number, env?: string | null) => Callme.curl(root(), file, text, index, opt(env)),
  copy: (text: string) => Callme.copy(text),
  envView: (env?: string | null): EnvView => JSON.parse(Callme.envView(root(), opt(env))),
  setEnv: (env: string) => Callme.setEnv(root(), env),
  history: (file: string, id: string, limit = 20): Run[] => JSON.parse(Callme.history(root(), file, id, limit)),
  filter: (json: string, path: string): unknown[] => JSON.parse(Callme.filter(json, path)),
  importText: (input: string, env?: string | null, name?: string | null): { text: string; replaced: string[] } =>
    JSON.parse(Callme.import(root(), input, opt(env), opt(name))),
  async run(file: string, id: string | null, env: string | null, onMessage: (m: { type: string; [k: string]: unknown }) => void): Promise<Run> {
    const out = await Callme.run(root(), file, id, env, (message) => onMessage(JSON.parse(message)));
    return JSON.parse(out);
  },
  pretty: (json: string) => Callme.pretty(json),
};
