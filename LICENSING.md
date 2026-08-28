# Licensing

albert is distributed under the **Business Source License 1.1 (BSL 1.1)** — see
[`LICENSE`](LICENSE). This is a *source-available* license, not an open-source
one, with a built-in expiry: **each released version becomes Apache 2.0 four
years after it ships.**

## What you can do

- **Read, modify, and build the source.**
- **Run it in production** — for yourself, your team, or your company.
- **Embed and integrate albert as a component, library, or tool inside your own
  applications, products, and internal systems** — including things you sell.
  If albert is a part of what you ship, that is fine.
- **Self-host it internally** with no restriction.

## The one thing you can't do

- **Operate albert itself as a service for third parties** — you may not offer
  albert (or a fork, or a derivative whose value is essentially albert's) as a
  hosted, managed, or multi-tenant / SaaS platform to others.

The line is: *build **with** albert freely; don't stand up an albert **platform**
in place of ours.* If you want to do exactly that, contact us for a commercial
license — lamantin-ai@yandex.ru.

## It becomes open source on a timer

Four years after a given version is published, that version automatically
converts to the **Apache License 2.0** (the "Change License"). BSL is
source-available now and fully open later — the restriction is on the newest
code, not forever.

## Maintainer note — the Change Date

The `Change Date` in `LICENSE` is the date *this checkout's* version opens.
BSL 1.1 also caps every version at four years from its own first publication
("whichever comes first"), so a version can never be locked longer than four
years even if the date is not bumped. Convention for releases: on each release,
set `Change Date` to that release's date + 4 years, so every version gets its
full and exact four-year window.

## Contributing

Contributions are accepted under the [Contributor License Agreement](CLA.md),
which lets the project keep its licensing coherent — including the four-year
conversion and any commercial licensing. See `CLA.md` for how to sign.

## Dependencies

albert pulls **octo** (MIT) and **kaeru** (BSL 1.1) — both LamantinAI's own — as
git dependencies. Through kaeru it includes **CozoDB**, which is **MPL-2.0**, a
file-level (weak) copyleft that permits inclusion in a larger work under a
different license; it is consumed unmodified. The remainder of the tree is
MIT / Apache-2.0. Nothing in the dependency tree forces albert open.

## History

albert has had no tagged releases; earlier development was under the MIT License.
The move to BSL applies from the relicensing commit forward — it is not
retroactive, and any copy obtained earlier under MIT remains available under it.
