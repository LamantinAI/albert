use std::{
    env::temp_dir,
    fs::{create_dir_all, remove_dir_all, write},
    path::PathBuf,
};

use tempfile::tempdir;

use super::{discovery::meta_of, scan, SkillStore};

/// A throwaway `skills/` tree; `skills` is a list of `(folder, SKILL.md body)`.
fn skills_dir(tag: &str, skills: &[(&str, &str)]) -> PathBuf {
    let root = temp_dir().join(format!("albert-skills-test-{tag}"));
    let _ = remove_dir_all(&root);
    for (name, body) in skills {
        create_dir_all(root.join(name)).unwrap();
        write(root.join(name).join("SKILL.md"), body).unwrap();
    }
    root
}

#[test]
fn adding_an_eleventh_skill_preserves_every_trigger_without_loading_bodies() {
    let dir = tempdir().unwrap();
    for count in 1..=35 {
        let name = format!("skill-{count:02}");
        let path = dir.path().join(&name);
        create_dir_all(&path).unwrap();
        write(path.join("SKILL.md"), format!(
            "---\nname: {name}\ndescription: Проверяй устройство device-{count:02}\n---\nPRIVATE_RECIPE_{count:02}"
        )).unwrap();
        if ![10, 11, 35].contains(&count) {
            continue;
        }
        let store = SkillStore::load(dir.path().to_path_buf(), 5, 10, &[]);
        let catalog = store.catalog();
        for index in 1..=count {
            assert!(
                catalog.contains(&format!(
                    "- skill-{index:02}: Проверяй устройство device-{index:02}"
                )),
                "missing skill {index} with {count} installed: {catalog}"
            );
        }
        assert!(!catalog.contains("PRIVATE_RECIPE"));
        let first = store.list_json(1);
        assert_eq!(first["skills"].as_array().unwrap().len(), 10);
        assert_eq!(first["total"], count);
        if count > 10 {
            assert_eq!(store.list_json(2)["skills"][0]["name"], "skill-11");
            assert_eq!(
                store.search_json("device-11", 10)["skills"][0]["name"],
                "skill-11"
            );
            assert_eq!(
                store.instructions("skill-11").unwrap().0,
                "PRIVATE_RECIPE_11"
            );
        }
        assert_eq!(
            catalog,
            SkillStore::load(dir.path().to_path_buf(), 5, 1, &[]).catalog()
        );
    }
}

#[test]
fn skills_declare_commands_reserved_and_duplicate_ones_are_refused() {
    let dir = skills_dir(
        "commands",
        &[
            ("brief", "---\nname: brief\ndescription: when asked for a brief\ncommand: brief\ncommand_about: Your day at a glance\n---\nDo the brief."),
            ("config", "---\nname: config\ndescription: change setup\ncommand: /settings\ncommand_owner: true\n---\nbody"),
            ("halt", "---\nname: halt\ndescription: d\ncommand: cancel\n---\nbody"),
            ("shout", "---\nname: shout\ndescription: d\ncommand: Loud-Name\n---\nbody"),
            ("zbrief", "---\nname: zbrief\ndescription: d\ncommand: brief\n---\nbody"),
        ],
    );
    let store = SkillStore::load(dir.clone(), 5, 10, &[]);
    let commands = store.commands();
    let names: Vec<(&str, &str, bool)> = commands
        .iter()
        .map(|c| (c.name.as_str(), c.skill.as_str(), c.owner))
        .collect();
    assert_eq!(
        names,
        [("brief", "brief", false), ("settings", "config", true)]
    );
    assert_eq!(commands[0].about, "Your day at a glance");
    assert_eq!(
        store.command("settings").map(|c| c.skill),
        Some("config".to_string())
    );
    assert_eq!(store.instructions("brief").unwrap().0, "Do the brief.");
    let _ = remove_dir_all(&dir);
}

#[test]
fn always_is_parsed_from_frontmatter() {
    assert!(meta_of("---\nname: s\nalways: true\n---\nbody").always);
    assert!(!meta_of("---\nname: s\ndescription: d\n---\nbody").always);
}

#[test]
fn an_always_skill_is_in_force_not_offered() {
    let dir = skills_dir(
        "always",
        &[
            ("style", "---\nname: style\nalways: true\ndescription: how to write\n---\nLead with the next action."),
            ("brief", "---\nname: brief\ndescription: pick me when asked\n---\nbody"),
        ],
    );
    let catalog = SkillStore::load(dir.clone(), 5, 10, &[]).catalog();

    // In force: the body itself is present, so no tool call is needed to obey it.
    assert!(
        catalog.contains("Lead with the next action."),
        "got:\n{catalog}"
    );
    assert!(catalog.contains("STANDING INSTRUCTIONS"), "got:\n{catalog}");
    // Not offered: absent from the pick-one list, which still holds the others.
    assert!(
        !catalog.contains("- style:"),
        "an in-force skill must not be offered:\n{catalog}"
    );
    assert!(
        catalog.contains("- brief: pick me when asked"),
        "got:\n{catalog}"
    );

    let _ = remove_dir_all(&dir);
}

#[test]
fn standing_instructions_and_skill_triggers_survive_small_list_pages() {
    // Small tool pages must hide neither standing instructions nor selectable skills.
    let dir = skills_dir(
        "always-paged",
        &[
            (
                "style",
                "---\nname: style\nalways: true\n---\nAction first.",
            ),
            ("a", "---\nname: a\ndescription: x\n---\nb"),
            ("b", "---\nname: b\ndescription: y\n---\nb"),
        ],
    );
    let catalog = SkillStore::load(dir.clone(), 5, 1, &[]).catalog();
    assert!(catalog.contains("Action first."), "got:\n{catalog}");
    assert!(
        catalog.contains("2 installed"),
        "in-force skills aren't counted:\n{catalog}"
    );
    assert!(catalog.contains("- a: x"));
    assert!(catalog.contains("- b: y"));
    let _ = remove_dir_all(&dir);
}

#[test]
fn a_skill_is_hidden_until_its_capability_is_present() {
    let dir = skills_dir(
        "gating",
        &[
            (
                "transcribe",
                "---\nname: transcribe\nrequires: subscription\n---\nbody",
            ),
            (
                "brief",
                "---\nname: brief\ndescription: always here\n---\nbody",
            ),
        ],
    );
    let catalog = SkillStore::load(dir.clone(), 5, 1, &[]).catalog();
    assert!(catalog.contains("- brief: always here"));
    assert!(!catalog.contains("transcribe"));
    let without: Vec<String> = scan(&dir, &[]).into_iter().map(|s| s.name).collect();
    assert_eq!(
        without,
        vec!["brief"],
        "no subscription -> transcribe is absent"
    );
    let with: Vec<String> = scan(&dir, &["subscription"])
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(
        with,
        vec!["brief", "transcribe"],
        "subscription -> both, sorted"
    );
    let _ = remove_dir_all(&dir);
}
