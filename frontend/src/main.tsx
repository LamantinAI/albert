import { createRoot } from "react-dom/client";

import { App } from "./app/App";
import "./styles/fonts.css";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/brand.css";
import "./styles/loading.css";
import "./styles/auth.css";
import "./styles/chat.css";
import "./styles/directory.css";
import "./styles/composer.css";
import "./styles/welcome.css";
import "./styles/admin.css";
import "./styles/overlays.css";
import "./styles/subscription.css";
import "./styles/progress.css";

createRoot(document.getElementById("root")!).render(<App />);
