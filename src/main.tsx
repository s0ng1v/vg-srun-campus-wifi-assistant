import React from "react";
import ReactDOM from "react-dom/client";
import { ThemeProvider } from "@/components/theme-provider";
import "./vendor/lens-design-system/tokens.css";
import "./vendor/lens-design-system/components.css";
import "./vendor/lens-design-system/showcase.css";
import "./vendor/lens-design-system/showcase-records.css";
import "./styles/lens-original.css";
import LensOriginalApp from "./LensOriginalApp";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ThemeProvider defaultTheme="light" storageKey="vg-campus-theme">
      <LensOriginalApp />
    </ThemeProvider>
  </React.StrictMode>,
);
