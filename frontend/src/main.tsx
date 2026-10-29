import { createRoot } from "react-dom/client";

import { App } from "./app/App";
import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/chat.css";

createRoot(document.getElementById("root")!).render(<App />);
