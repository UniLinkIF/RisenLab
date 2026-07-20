import ReactDOM from "react-dom/client";
import App from "./App";
import { initRemoteToken } from "./lib/remoteToken";
import "./theme.css";

initRemoteToken();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
