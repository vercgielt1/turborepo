import * as React from "react";

import type { Issue } from "@vercel/turbopack-runtime/types/protocol";

import * as Bus from "./bus";
import { ShadowPortal } from "./components/ShadowPortal";
import { Errors, SupportedErrorEvent } from "./container/Errors";
import { ErrorBoundary } from "./ErrorBoundary";
import { Base } from "./styles/Base";
import { ComponentStyles } from "./styles/ComponentStyles";
import { CssReset } from "./styles/CssReset";

type RefreshState =
  | {
      // No refresh in progress.
      type: "idle";
    }
  | {
      // The refresh process has been triggered, but the new code has not been
      // executed yet.
      type: "pending";
      errors: SupportedErrorEvent[];
    };

type OverlayState = {
  nextId: number;

  // issues are from turbopack
  issues: Issue[];

  // errors are client side
  errors: SupportedErrorEvent[];

  refreshState: RefreshState;
};

function pushErrorFilterDuplicates(
  errors: SupportedErrorEvent[],
  err: SupportedErrorEvent
): SupportedErrorEvent[] {
  return [
    ...errors.filter((e) => {
      // Filter out duplicate errors
      return e.event.reason !== err.event.reason;
    }),
    err,
  ];
}

function reducer(state: OverlayState, ev: Bus.BusEvent): OverlayState {
  console.log("error");
  switch (ev.type) {
    case Bus.TYPE_BUILD_OK: {
      return { ...state };
    }
    case Bus.TYPE_TURBOPACK_ISSUES: {
      return { ...state, issues: ev.issues };
    }
    case Bus.TYPE_BEFORE_REFRESH: {
      return { ...state, refreshState: { type: "pending", errors: [] } };
    }
    case Bus.TYPE_REFRESH: {
      return {
        ...state,
        errors:
          // Errors can come in during updates. In this case, UNHANDLED_ERROR
          // and UNHANDLED_REJECTION events might be dispatched between the
          // BEFORE_REFRESH and the REFRESH event. We want to keep those errors
          // around until the next refresh. Otherwise we run into a race
          // condition where those errors would be cleared on refresh completion
          // before they can be displayed.
          state.refreshState.type === "pending"
            ? state.refreshState.errors
            : [],
        refreshState: { type: "idle" },
      };
    }
    case Bus.TYPE_UNHANDLED_ERROR:
    case Bus.TYPE_UNHANDLED_REJECTION: {
      switch (state.refreshState.type) {
        case "idle": {
          return {
            ...state,
            nextId: state.nextId + 1,
            errors: pushErrorFilterDuplicates(state.errors, {
              id: state.nextId,
              event: ev,
            }),
          };
        }
        case "pending": {
          return {
            ...state,
            nextId: state.nextId + 1,
            refreshState: {
              ...state.refreshState,
              errors: pushErrorFilterDuplicates(state.refreshState.errors, {
                id: state.nextId,
                event: ev,
              }),
            },
          };
        }
        default:
          return state;
      }
    }
    default: {
      return state;
    }
  }
}

type ErrorType = "runtime" | "build";

const shouldPreventDisplay = (
  errorType?: ErrorType | null,
  preventType?: ErrorType[] | null
) => {
  if (!preventType || !errorType) {
    return false;
  }
  return preventType.includes(errorType);
};

type ReactDevOverlayProps = {
  globalOverlay?: boolean;
  preventDisplay?: ErrorType[];
  children?: React.ReactNode;
};

export default function ReactDevOverlay({
  children,
  preventDisplay,
  globalOverlay,
}: ReactDevOverlayProps) {
  const [state, dispatch] = React.useReducer<
    React.Reducer<OverlayState, Bus.BusEvent>
  >(reducer, {
    nextId: 1,
    issues: [],
    errors: [],
    refreshState: {
      type: "idle",
    },
  });

  React.useEffect(() => {
    Bus.on(dispatch);
    return function () {
      Bus.off(dispatch);
    };
  }, [dispatch]);

  const onComponentError = React.useCallback(
    (_error: Error, _componentStack: string | null) => {
      // TODO: special handling
    },
    []
  );

  const hasBuildError = state.issues.length > 0;
  const hasRuntimeErrors = state.errors.length > 0;

  const errorType = hasBuildError
    ? "build"
    : hasRuntimeErrors
    ? "runtime"
    : null;

  const isMounted = hasBuildError || hasRuntimeErrors;

  return (
    <React.Fragment>
      <ErrorBoundary
        globalOverlay={globalOverlay}
        isMounted={isMounted}
        onError={onComponentError}
      >
        {children ?? null}
      </ErrorBoundary>
      {isMounted ? (
        <ShadowPortal globalOverlay={globalOverlay}>
          <CssReset />
          <Base />
          <ComponentStyles />

          {shouldPreventDisplay(errorType, preventDisplay) ? null : (
            <Errors issues={state.issues} errors={state.errors} />
          )}
        </ShadowPortal>
      ) : null}
    </React.Fragment>
  );
}
