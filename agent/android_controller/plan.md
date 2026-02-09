# Plan: Android Command Agent (MVP)

## 1. Goal

Translate natural language intent into a sequence of executable Android MCP tool calls.

## 2. In-Scope (MVP)

* **App Management:** Launching specific apps by package name.
* **UI Interaction:** Tapping coordinates, typing text, and swiping.
* **Read State:** Getting the current view hierarchy or a screenshot to verify success.
* **Control:** Hard allowlist of packages the agent is allowed to touch (e.g., only Communication apps).

## 3. Artifacts to Produce

* **Execution Log:** A JSON array of every tool call attempted and the device response.
* **Final State:** A screenshot or UI dump confirming the final screen reached.

## 4. Execution Steps

### Step 1: Intent Mapping

The agent receives the prompt and queries the MCP server for the list of available tools (e.g., `android:tap`, `android:type_text`, `android:launch_app`).

### Step 2: Plan Generation (Internal)

The LLM generates a JSON sequence of actions.

* *Example:* 1. `launch_app(pkg="com.whatsapp")`
2. `wait(2s)`
3. `tap(x=500, y=100)` // Search bar

### Step 3: Gated Execution

Hugind executes the calls one by one. After each "tap" or "type," the agent must check the UI state to ensure the phone reacted as expected before moving to the next command.

### Step 4: Verification

The agent captures the current screen. If the text "Omar" is visible in the UI hierarchy, the task is marked **SUCCESS**.

---

## 5. Safety & Constraints

* **Timeout:** If the goal isn't reached in 10 steps, kill the process.
* **Permissions:** `agent.yaml` will strictly deny network access to ensure no data from the phone is exfiltrated.
* **Resource Cap:** Limit CPU/RAM to prevent the agent from hanging the host during a loop.

---

### How the `agent.yaml` would look:

```yaml
name: "droid_controller"
permissions:
  network:
    allow: false
  filesystem:
    allow: true # To save screenshots for the audit trail
  shell:
    allow: false # Everything goes through MCP, not raw ADB shell
dependencies:
  mcp:
    - name: "android-mcp"
      command: "python"
      args: ["-m", "phone_mcp"]

```
