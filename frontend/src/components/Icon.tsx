export type IconName =
  | "plus"
  | "chats"
  | "models"
  | "skills"
  | "settings"
  | "users"
  | "arrow"
  | "chevron"
  | "close"
  | "menu"
  | "stop"
  | "check"
  | "search"
  | "logout"
  | "temporary"
  | "copy";

const paths: Record<IconName, string> = {
  plus: "M12 5v14M5 12h14",
  chats: "M5 4h14a1 1 0 0 1 1 1v12H9l-5 3V5a1 1 0 0 1 1-1Z M8 9h8M8 13h5",
  models: "m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5",
  skills: "M5 4h11l3 3v13H5V4Zm10 0v5h4M8 13h8M8 17h5",
  settings: "M4 7h16M4 17h16M8 4v6M16 14v6",
  users:
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M17 4a4 4 0 0 1 0 8M22 21v-2a4 4 0 0 0-3-4M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
  arrow: "M5 12h14m-6-6 6 6-6 6",
  chevron: "m9 5 7 7-7 7",
  close: "m6 6 12 12M6 18 18 6",
  menu: "M4 6h16M4 12h16M4 18h16",
  stop: "M6 6h12v12H6z",
  check: "m5 12 4 4L19 6",
  search: "m16 16 5 5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z",
  logout: "M9 4H4v16h5M9 12h12m-5-5 5 5-5 5",
  temporary:
    "M8 3h8M8 21h8M7 4c0 5 1 6 5 8-4 2-5 3-5 8M17 4c0 5-1 6-5 8 4 2 5 3 5 8",
  copy: "M8 8h12v13H8zM4 16V3h12",
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
