import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { MemoryRouter } from "react-router-dom";
import { SettingsList, SettingsRow, SettingsRowDetails } from "./SettingsList";

test("navigation and managed rows use native links and separate actions", () => {
  const edit = vi.fn();
  render(
    <MemoryRouter>
      <SettingsList label="Settings">
        <SettingsRow
          kind="link"
          to="/models"
          title="Models"
          description="Model preferences"
        />
        <SettingsRow
          title="Provider key"
          actions={<button onClick={edit}>Replace</button>}
        />
      </SettingsList>
    </MemoryRouter>,
  );
  expect(screen.getByRole("list", { name: "Settings" })).toBeVisible();
  const link = screen.getByRole("link");
  expect(link).toHaveAttribute("href", "/models");
  expect(link.querySelector("button")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Replace" }));
  expect(edit).toHaveBeenCalledTimes(1);
});

test("radio details remain outside the label and unavailable reasons describe their option", () => {
  const select = vi.fn();
  const { container } = render(
    <SettingsList selection label="Models">
      <SettingsRow
        kind="radio"
        title="Balanced"
        description="Everyday tasks"
        note="Unavailable for this account"
        radio={{
          name: "models",
          value: "balanced",
          checked: false,
          disabled: true,
          label: "Balanced",
          onChange: select,
        }}
        details={
          <SettingsRowDetails ariaLabel="Balanced details">
            Model metadata
          </SettingsRowDetails>
        }
      />
    </SettingsList>,
  );
  const radio = screen.getByRole("radio", { name: "Balanced" });
  expect(radio).toBeDisabled();
  expect(radio).toHaveAccessibleDescription(
    "Everyday tasks Unavailable for this account",
  );
  expect(container.querySelector("label details")).toBeNull();
  fireEvent.click(screen.getByText("Details"));
  expect(select).not.toHaveBeenCalled();
});
