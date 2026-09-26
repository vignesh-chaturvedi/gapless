import { RotateCw } from "lucide-react";
import { Component, type ErrorInfo, type ReactNode } from "react";

import { Button } from "@/components/ui/button";

interface State {
  error: Error | null;
}

/** Keeps one broken view from blanking the whole console. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("gapless console:", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div role="alert" className="mx-auto flex max-w-xl flex-col items-start gap-4 px-4 py-20 lg:px-6">
        <p className="label text-gap">Console error</p>
        <h1 className="text-2xl font-semibold tracking-[-0.025em]">This view stopped rendering</h1>
        <p className="text-muted-foreground">
          The stream and the server are unaffected. Reload the console to pick up where it left off.
        </p>
        <pre className="num w-full overflow-x-auto rounded-md border border-hairline bg-panel px-3 py-2 text-xs text-muted-foreground">
          {this.state.error.message}
        </pre>
        <Button onClick={() => window.location.reload()}>
          <RotateCw aria-hidden="true" />
          Reload
        </Button>
      </div>
    );
  }
}
