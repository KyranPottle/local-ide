import { render } from "solid-js/web"
import "./design/fonts.css"
import "./design/tokens.css"
import "./design/base.css"
import { App } from "./App"

const root = document.getElementById("root")
if (!root) throw new Error("index.html has no #root element")

render(() => <App />, root)
