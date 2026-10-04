// A tiny pub/sub so the query client (created outside React) can tell the
// auth gate that the session is gone. Not a state store: it holds no state.

type Listener = () => void;

const listeners = new Set<Listener>();

export function onSessionExpired(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function notifySessionExpired(): void {
  for (const listener of listeners) listener();
}
