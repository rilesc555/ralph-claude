/**
 * OpenCode Ralph Stop-Hook Plugin
 *
 * This plugin monitors opencode's session state and writes a signal file
 * when the session becomes idle (agent finishes processing). The ralph-uv
 * loop runner watches this signal file to detect iteration completion.
 *
 * Environment variables:
 *   RALPH_SIGNAL_FILE - Path to the JSON signal file to write on idle
 *   RALPH_SESSION_ID  - Optional session identifier for the signal payload
 *
 * Signal file format (JSON):
 *   { "event": "idle", "timestamp": "<ISO 8601>", "session_id": "<id>" }
 *
 * The plugin is loaded by opencode via the .opencode/plugins/ directory
 * mechanism. It hooks into the session lifecycle to detect when the agent
 * finishes processing a request.
 */

import * as fs from "fs";
import * as path from "path";

/** Signal payload written to the signal file on session idle. */
interface IdleSignal {
  event: "idle";
  timestamp: string;
  session_id: string;
}

/** Plugin configuration from environment. */
interface PluginConfig {
  signalFile: string;
  sessionId: string;
}

/**
 * Read plugin configuration from environment variables.
 * Returns null if RALPH_SIGNAL_FILE is not set (plugin is a no-op).
 */
function getConfig(): PluginConfig | null {
  const signalFile = process.env.RALPH_SIGNAL_FILE;
  if (!signalFile) {
    return null;
  }

  return {
    signalFile,
    sessionId: process.env.RALPH_SESSION_ID || "unknown",
  };
}

/**
 * Write the idle signal to the configured signal file.
 * Creates parent directories if they don't exist.
 * Writes atomically (write to temp + rename) to prevent partial reads.
 */
function writeSignal(config: PluginConfig): void {
  const signal: IdleSignal = {
    event: "idle",
    timestamp: new Date().toISOString(),
    session_id: config.sessionId,
  };

  const content = JSON.stringify(signal, null, 2) + "\n";
  const dir = path.dirname(config.signalFile);

  // Ensure directory exists
  fs.mkdirSync(dir, { recursive: true });

  // Write atomically: temp file + rename
  const tmpFile = config.signalFile + ".tmp";
  fs.writeFileSync(tmpFile, content, { mode: 0o644 });
  fs.renameSync(tmpFile, config.signalFile);
}

/**
 * OpenCode Plugin Entry Point
 *
 * Uses the opencode plugin API:
 * - Plugin is an async function that receives context
 * - Returns an object with hook handlers
 * - The `event` hook receives all opencode events
 * - We listen for `session.idle` to signal completion to ralph-uv
 *
 * Plugin lifecycle:
 * 1. opencode loads the plugin on startup
 * 2. Plugin registers an event handler
 * 3. When agent finishes processing, opencode fires session.idle
 * 4. Plugin writes the signal file
 * 5. ralph-uv detects the signal file and terminates the process
 */
export const RalphHook = async (ctx: any) => {
  const config = getConfig();
  if (!config) {
    // RALPH_SIGNAL_FILE not set - plugin is a no-op
    // This allows the plugin to be installed globally without side effects
    return {};
  }

  if (process.env.RALPH_DEBUG) {
    console.error("[ralph-hook] Plugin loaded, watching for session.idle");
    console.error("[ralph-hook] Signal file:", config.signalFile);
  }

  return {
    event: async ({ event }: { event: { type: string; properties?: any } }) => {
      if (event.type === "session.idle") {
        try {
          writeSignal(config);
          if (process.env.RALPH_DEBUG) {
            console.error("[ralph-hook] Signal written:", config.signalFile);
          }
        } catch (err) {
          // Silently fail - don't crash opencode if signal write fails
          // ralph-uv has a fallback (process exit detection)
          if (process.env.RALPH_DEBUG) {
            console.error("[ralph-hook] Failed to write signal:", err);
          }
        }
      }
    },
  };
};
