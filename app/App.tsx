// callme GUI prototype (M0): the project's requests on the left, the selected file in an editor with callme's
// `.http` highlighter, and the response on the right, server-sent events streamed in as they arrive.
import { mount, signal, Scroll, Text, View } from "nsh";
import { Editor } from "nsh/base";
import { Callme } from "callme/native";

type RequestRef = { file: string; id: string; name?: string; index: number; method: string; url: string };
type Run = {
  request: { method: string; url: string; env?: string };
  response: { status: number; reason: string; httpVersion: string; headers: Record<string, string | string[]>; bodyKind: string; body: unknown; bodyTruncated: boolean; size: number };
  timing: { dns: number; connect: number; tls: number; ttfb: number; download: number; total: number };
};

const root = Callme.root();

function errorMessage(e: unknown): string {
  const text = e instanceof Error ? e.message : String(e);
  try {
    const { error } = JSON.parse(text);
    return error.hint ? `${error.message}\n${error.hint}` : error.message;
  } catch {
    return text;
  }
}

function load(): RequestRef[] {
  try {
    return JSON.parse(Callme.list(root));
  } catch {
    return [];
  }
}

const methodColor: Record<string, string> = { GET: "text-teal-600", POST: "text-orange-600", PUT: "text-blue-600", DELETE: "text-red-600" };

function App() {
  const requests = signal<RequestRef[]>(load());
  const first = requests()[0];
  const file = signal(first?.file ?? "");
  const selected = signal(first?.id ?? "");
  const text = signal(first ? Callme.read(root, first.file) : "");
  const run = signal<Run | null>(null);
  const events = signal<unknown[]>([]);
  const status = signal("");
  const running = signal(false);

  const open = (r: RequestRef) => {
    if (r.file !== file()) {
      file.set(r.file);
      text.set(Callme.read(root, r.file));
    }
    selected.set(r.id);
  };

  const send = async () => {
    if (!file() || running()) return;
    running.set(true);
    run.set(null);
    events.set([]);
    status.set("发送中…");
    try {
      Callme.write(root, file(), text());
      requests.set(load());
      const out = await Callme.run(root, file(), selected() || null, null, (message) => {
        const m = JSON.parse(message);
        if (m.type === "head" && m.streaming) status.set(`${m.status} · 流式接收中…`);
        if (m.type === "event") events.set([...events(), m.event]);
      });
      run.set(JSON.parse(out));
      status.set("");
    } catch (e) {
      status.set(errorMessage(e));
    }
    running.set(false);
  };

  const bodyText = () => {
    const r = run();
    if (!r) return "";
    const b = r.response.body;
    return typeof b === "string" ? b : JSON.stringify(b, null, 2);
  };
  const bodyFolds = () => JSON.parse(Callme.jsonFolds(bodyText()));

  return (
    <View class="flex-1 flex-row bg-background">
      <View class="w-60 border-r border-border bg-muted/30 pt-10">
        <Text class="px-4 pb-2 text-xs font-semibold text-muted-foreground">请求</Text>
        <Scroll class="flex-1">
          <Each of={requests()}>
            {(r) => (
              <View
                role="button"
                onPress={() => open(r())}
                class={`mx-2 flex-row gap-2 rounded-md px-2 py-1.5 ${r().file === file() && r().id === selected() ? "bg-primary/15" : ""}`}
              >
                <Text class={`w-12 font-mono text-xs font-bold ${methodColor[r().method] ?? ""}`}>{r().method}</Text>
                <Text class="flex-1 text-sm" style={{ numberOfLines: 1 }}>{r().name ?? `${r().file}#${r().index}`}</Text>
              </View>
            )}
          </Each>
        </Scroll>
      </View>

      <View class="flex-1">
        <View class="h-12 flex-row items-center gap-3 border-b border-border px-3">
          <Text class="flex-1 text-sm text-muted-foreground" style={{ numberOfLines: 1 }}>{file() ? `${file()} · ${selected()}` : `${root} 里没有 .http 文件`}</Text>
          <View role="button" onPress={send} class={`rounded-md px-4 py-1.5 ${running() ? "bg-muted" : "bg-foreground active:bg-foreground/80"}`}>
            <Text class={running() ? "text-muted-foreground" : "text-background font-semibold"}>{running() ? "发送中" : "发送"}</Text>
          </View>
        </View>
        <Editor
          value={text()}
          onChange={({ value }) => text.set(value)}
          language="http"
          foldRanges={JSON.parse(Callme.httpFolds(text()))}
          class="flex-1"
        />
      </View>

      <View class="w-[480px] border-l border-border">
        <View class="h-12 flex-row items-center gap-3 border-b border-border px-3">
          <If when={run()}>
            {(r) => (
              <>
                <Text class={`font-mono font-bold ${r().response.status < 400 ? "text-green-600" : "text-red-600"}`}>
                  {`${r().response.status} ${r().response.reason}`}
                </Text>
                <Text class="font-mono text-xs text-muted-foreground">{`${r().timing.total} ms · ${r().response.size} B · ${r().response.httpVersion}`}</Text>
              </>
            )}
          </If>
          <Text class="flex-1 text-sm text-muted-foreground" style={{ numberOfLines: 2 }}>{status()}</Text>
        </View>
        <If when={events().length > 0}>
          <Scroll class="max-h-60 border-b border-border">
            <Each of={events()}>
              {(e) => <Text class="px-3 py-1 font-mono text-xs">{JSON.stringify(e())}</Text>}
            </Each>
          </Scroll>
        </If>
        <Editor value={bodyText()} language="json" readOnly foldRanges={bodyFolds()} class="flex-1" />
      </View>
    </View>
  );
}

mount(App);
