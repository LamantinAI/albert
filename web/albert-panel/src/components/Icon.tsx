export type IconName =
  | "plus"
  | "chats"
  | "models"
  | "skills"
  | "settings"
  | "users"
  | "user"
  | "key"
  | "book"
  | "plug"
  | "robot"
  | "arrow"
  | "chevron"
  | "close"
  | "menu"
  | "more"
  | "stop"
  | "check"
  | "search"
  | "logout"
  | "temporary"
  | "copy"
  | "terminal"
  | "tool"
  | "spark"
  | "plan"
  | "shield";

const paths: Record<IconName, string> = {
  plus: "M12 5v14M5 12h14",
  chats: "M5 4h14a1 1 0 0 1 1 1v12H9l-5 3V5a1 1 0 0 1 1-1Z M8 9h8M8 13h5",
  models: "m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5",
  skills: "M5 4h11l3 3v13H5V4Zm10 0v5h4M8 13h8M8 17h5",
  settings: "M4 7h16M4 17h16M8 4v6M16 14v6",
  users:
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M17 4a4 4 0 0 1 0 8M22 21v-2a4 4 0 0 0-3-4M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
  user: "M20 21v-2a8 8 0 0 0-16 0v2M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
  key: "M11.5 7a4.5 4.5 0 1 1-9 0 4.5 4.5 0 0 1 9 0ZM10.2 10.2 21 21M17 17l3-3M14 14l3-3",
  book: "M12 5C9 3 5 3 2 4v15c3-1 7-1 10 1 3-2 7-2 10-1V4c-3-1-7-1-10 1Zm0 0v15M5 8h4M5 11h4M15 8h4M15 11h4",
  plug: "M8 2v5M16 2v5M6 7h12v4a6 6 0 0 1-12 0V7Zm6 10v5",
  robot:
    "M13 2a1 1 0 1 1-2 0 1 1 0 0 1 2 0ZM12 3v3M7 6h10a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2ZM2 10v7M22 10v7M9 11h.01M15 11h.01M8 16h8M10 15v2M14 15v2",
  arrow: "M5 12h14m-6-6 6 6-6 6",
  chevron: "m9 5 7 7-7 7",
  close: "m6 6 12 12M6 18 18 6",
  menu: "M4 6h16M4 12h16M4 18h16",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  stop: "M6 6h12v12H6z",
  check: "m5 12 4 4L19 6",
  search: "m16 16 5 5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z",
  logout: "M9 4H4v16h5M9 12h12m-5-5 5 5-5 5",
  temporary:
    "M8 3h8M8 21h8M7 4c0 5 1 6 5 8-4 2-5 3-5 8M17 4c0 5-1 6-5 8 4 2 5 3 5 8",
  copy: "M8 8h12v13H8zM4 16V3h12",
  terminal: "M3 4h18v16H3zM7 9l3 3-3 3M13 15h4",
  tool: "m14 6 4 4 3-3a7 7 0 0 1-9 9l-6 6-4-4 6-6a7 7 0 0 1 9-9l-3 3Z",
  spark: "m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3Z",
  plan: "M4 4h16v16H4zM8 9h1M12 9h4M8 15h1M12 15h4",
  shield: "m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Zm-4 9 3 3 5-6",
};

export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]} />
    </svg>
  );
}
