import React from "react";
import ReactDOM from "react-dom/client";
import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import App from "./App";

// Keep the splash up long enough for its 200ms reveal to finish, then fade it
// out once the first app frame is on screen.
const SPLASH_MIN_VISIBLE_MS = 260;
const SPLASH_FADE_MS = 200;
const splash = document.getElementById("pz-splash");
const splashShownAt = performance.now();

function dismissSplash() {
  if (!splash) {
    return;
  }
  const elapsed = performance.now() - splashShownAt;
  window.setTimeout(() => {
    splash.classList.add("is-hiding");
    window.setTimeout(() => splash.remove(), SPLASH_FADE_MS);
  }, Math.max(0, SPLASH_MIN_VISIBLE_MS - elapsed));
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ChakraProvider value={defaultSystem}>
      <App />
    </ChakraProvider>
  </React.StrictMode>,
);

// Two frames: one for React to commit the tree, one for the browser to paint it.
window.requestAnimationFrame(() => window.requestAnimationFrame(dismissSplash));
