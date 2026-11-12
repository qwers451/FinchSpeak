/// Classify a foreground executable without exposing its path outside this process.
pub fn classify(exe_path: &str, covers_monitor: bool) -> Option<String> {
    let normalized = exe_path.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    let executable = normalized.rsplit('\\').next()?;
    let stem = if executable.to_ascii_lowercase().ends_with(".exe") {
        &executable[..executable.len() - 4]
    } else {
        executable
    };
    let name = stem.to_lowercase();
    let excluded = [
        "chrome",
        "msedge",
        "firefox",
        "opera",
        "brave",
        "vivaldi",
        "yandex",
        "browser",
        "vlc",
        "mpc-hc",
        "mpc-hc64",
        "mpv",
        "wmplayer",
        "explorer",
        "powerpnt",
        "obs64",
        "devenv",
        "code",
        "steam",
        "steamwebhelper",
        "epicgameslauncher",
        "galaxyclient",
        "battle.net",
        "riotclientservices",
        "msedgewebview2",
        "finch-app",
        "finchspeak",
    ];
    if stem.is_empty() || excluded.contains(&name.as_str()) {
        return None;
    }
    let segments = [
        "\\steamapps\\common\\",
        "\\epic games\\",
        "\\gog galaxy\\games\\",
        "\\gog games\\",
        "\\xboxgames\\",
        "\\riot games\\",
        "\\battle.net\\",
        "\\blizzard\\",
        "\\ubisoft game launcher\\games\\",
        "\\ea games\\",
        "\\origin games\\",
        "\\rockstar games\\",
    ];
    let known_directory = segments.iter().any(|segment| lower.contains(segment));
    let fullscreen_game = covers_monitor && !name.starts_with("potplayer");
    if !known_directory && !fullscreen_game {
        return None;
    }
    let steam_segment = "\\steamapps\\common\\";
    let fallback = lower
        .find(steam_segment)
        .and_then(|index| normalized.get(index + steam_segment.len()..))
        .and_then(|rest| rest.split('\\').next())
        .filter(|folder| !folder.is_empty())
        .unwrap_or(stem);
    Some(game_title(None, None, fallback))
}

pub fn game_title(description: Option<&str>, product: Option<&str>, fallback: &str) -> String {
    let title = description
        .filter(|text| !text.trim().is_empty())
        .or_else(|| product.filter(|text| !text.trim().is_empty()))
        .unwrap_or(fallback);
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_recognizes_all_launcher_paths_case_insensitively() {
        for segment in [
            "steamapps/common",
            "Epic Games",
            "GOG Galaxy/Games",
            "GOG Games",
            "XboxGames",
            "Riot Games",
            "Battle.net",
            "Blizzard",
            "Ubisoft Game Launcher/games",
            "EA Games",
            "Origin Games",
            "Rockstar Games",
        ] {
            let path = format!("C:/{segment}/The Game/game.exe");
            assert!(classify(&path.to_uppercase(), false).is_some(), "{segment}");
        }
        assert_eq!(
            classify(r"D:\Steam\steamapps\common\Portal 2\bin\portal2.exe", false),
            Some("Portal 2".into())
        );
        assert_eq!(classify(r"C:\Tools\unknown.exe", false), None);
    }

    #[test]
    fn activity_fullscreen_uses_exe_fallback_and_excludes_apps_and_launchers() {
        assert_eq!(
            classify(r"C:\Games\MyGame.exe", true),
            Some("MyGame".into())
        );
        for exe in [
            "chrome",
            "msedge",
            "firefox",
            "opera",
            "brave",
            "vivaldi",
            "yandex",
            "browser",
            "vlc",
            "mpc-hc",
            "mpc-hc64",
            "mpv",
            "PotPlayerMini64",
            "wmplayer",
            "explorer",
            "powerpnt",
            "obs64",
            "devenv",
            "code",
            "steam",
            "steamwebhelper",
            "epicgameslauncher",
            "galaxyclient",
            "battle.net",
            "riotclientservices",
            "msedgewebview2",
            "finch-app",
            "FinchSpeak",
        ] {
            assert_eq!(
                classify(&format!("C:/Tools/{exe}.exe"), true),
                None,
                "{exe}"
            );
        }
        assert_eq!(
            classify("C:/Epic Games/Launcher/epicgameslauncher.exe", false),
            None
        );
        assert_eq!(classify("C:/Epic Games/browser.exe", false), None);
        assert_eq!(classify("C:/Epic Games/msedgewebview2.exe", false), None);
    }

    #[test]
    fn launchers_in_game_directories_are_not_games() {
        assert_eq!(
            classify(r"C:\Program Files (x86)\Battle.net\Battle.net.exe", false),
            None
        );
        assert_eq!(
            classify(
                r"C:\Program Files\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe",
                false
            ),
            None
        );
        assert_eq!(
            classify(r"C:\Riot Games\Riot Client\RiotClientServices.exe", false),
            None
        );
        assert_eq!(
            classify(r"C:\Program Files (x86)\Steam\steam.exe", true),
            None
        );
        assert_eq!(
            classify(r"D:\SteamLibrary\steamapps\common\Enshrouded\enshrouded.exe", false),
            Some("Enshrouded".into())
        );
    }

    #[test]
    fn activity_title_uses_description_then_product_then_classified_fallback() {
        assert_eq!(
            game_title(Some("Portal 2"), Some("Source"), "portal2"),
            "Portal 2"
        );
        assert_eq!(
            game_title(Some("  "), Some("Portal 2"), "portal2"),
            "Portal 2"
        );
        assert_eq!(game_title(None, None, "portal2"), "portal2");
        assert_eq!(
            game_title(Some(&"я".repeat(80)), None, "fallback")
                .chars()
                .count(),
            64
        );
        assert_eq!(game_title(Some("One\nTwo"), None, "fallback"), "One Two");
    }
}
