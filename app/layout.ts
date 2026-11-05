// The panes' widths, as the user last dragged them, kept in the app's storage for the next launch.
import { signal, Storage } from "nsh";

/** The sidebar's width and the right-hand pane's (the response, or the environment's variables). */
const LAYOUT_KEY = "layout";
export const layout = signal({ sidebar: 248, response: 500 });
void Storage.getItem(LAYOUT_KEY).then((saved) => {
  try {
    const value = saved ? JSON.parse(saved) : null;
    if (value && typeof value.sidebar === "number" && typeof value.response === "number") layout.set(value);
  } catch {}
});
export function resized(panel: "sidebar" | "response", size: number) {
  const next = { ...layout(), [panel]: Math.round(size) };
  layout.set(next);
  void Storage.setItem(LAYOUT_KEY, JSON.stringify(next));
}

/** The line between two panes: a hairline that darkens under the pointer and turns the brand color while dragged. */
export const HANDLE = "bg-border group-hover:bg-muted-foreground group-active:bg-brand";
