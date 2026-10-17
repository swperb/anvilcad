# AnvilCAD notes for agents

- Read CONTRIBUTING.md for the engineering standards and the clean-room rules.
- To see or test the app, use the headless harness, never the windowed app:
  `cargo run -p anvil-app --bin anvilcad-agent -- ...` (docs/AGENT_HARNESS.md). It opens
  no window, so it never takes focus from the user or uses their mouse and keyboard.
  Running `anvilcad` itself (`cargo run -p anvil-app`) opens a real window; don't.
- Save harness screenshots under `target/` and look at them before you describe them.
