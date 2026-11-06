import type { components } from "../../api/schema";
import type { IconName } from "../../components/Icon";

type Activity = components["schemas"]["ProgressActivity"];
type Appearance = { icon: IconName; label: string };
const generic: Appearance = { icon: "tool", label: "Tool" };
const activities: Record<Activity, Appearance> = {
  scratchpad: { icon: "notebook", label: "Scratchpad" },
  memory: { icon: "brain", label: "Memory" },
  skill: { icon: "skills", label: "Skill" },
  connector: { icon: "plug", label: "Connector" },
  search: { icon: "search", label: "Web search" },
  browser: { icon: "globe", label: "Browser" },
  files: { icon: "folder", label: "Files" },
  shell: { icon: "terminal", label: "Shell" },
  sql: { icon: "database", label: "SQL" },
  calendar: { icon: "calendar", label: "Calendar" },
  mail: { icon: "mail", label: "Mail" },
  image: { icon: "image", label: "Image" },
  audio_input: { icon: "mic", label: "Transcription" },
  audio_output: { icon: "volume", label: "Speech" },
  subagent: { icon: "robot", label: "Subagent" },
  sessions: { icon: "chats", label: "Session" },
  configuration: { icon: "settings", label: "Configuration" },
  model: { icon: "models", label: "Model" },
  unknown: generic,
};

/** Labels are display content; only typed host metadata selects a tool icon. */
export function activityAppearance(item: {
  kind: string;
  activity?: unknown;
}): Appearance {
  if (item.kind === "stage") return { icon: "plan", label: "Step" };
  if (item.kind === "child") return { icon: "robot", label: "Subagent" };
  if (
    item.kind !== "tool" ||
    typeof item.activity !== "string" ||
    !Object.hasOwn(activities, item.activity)
  )
    return generic;
  return activities[item.activity as Activity];
}
