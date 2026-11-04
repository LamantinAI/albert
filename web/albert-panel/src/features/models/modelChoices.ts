import type { SettingsChoice } from "../../components/SettingsActions";

export function modelChoices(
  onApi: () => void,
  onSubscription: () => void,
): SettingsChoice[] {
  return [
    {
      title: "API",
      description:
        "Use an API key from OpenRouter or another compatible provider",
      icon: "key",
      onClick: onApi,
    },
    {
      title: "Subscription",
      description: "Choose a model using your ChatGPT subscription",
      icon: "models",
      onClick: onSubscription,
    },
  ];
}

export function chatReturnPath(value: string | null) {
  if (!value || value.includes("\\")) return "/chats";
  try {
    const url = new URL(value, "https://workspace.invalid");
    return url.origin === "https://workspace.invalid" &&
      (url.pathname === "/chats" || url.pathname.startsWith("/chats/"))
      ? url.pathname + url.search
      : "/chats";
  } catch {
    return "/chats";
  }
}
