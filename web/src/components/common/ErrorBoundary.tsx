// Generic error boundary: shows a fallback placeholder when a child throws while rendering, so the
// whole page never goes blank. It also shows the concrete error, making render failures easy to trace.
import { Component, type ErrorInfo, type ReactNode } from "react";
import { t as tr, tFormat } from "@/lib/localeDict";

interface Props {
  children: ReactNode;
  fallback?: (error: Error) => ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // eslint-disable-next-line no-console
    console.error("ErrorBoundary caught:", error, info);
  }

  render() {
    if (this.state.error) {
      if (this.props.fallback) return this.props.fallback(this.state.error);
      return (
        <div className="p-3 text-xs text-muted-foreground">
          <div>{tFormat("common.renderError", this.state.error.message)}</div>
          <button
            type="button"
            className="mt-1 text-primary hover:underline"
            onClick={() => this.setState({ error: null })}
          >
            {tr("common.retry")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
