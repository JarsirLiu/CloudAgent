use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-env-changed=CLOUDAGENT_GIT_DESCRIBE");

    if let Ok(override_value) = std::env::var("CLOUDAGENT_GIT_DESCRIBE")
        && !override_value.trim().is_empty()
    {
        println!("cargo:rustc-env=CLOUDAGENT_BUILD_VERSION={override_value}");
        return;
    }

    let describe = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let Some(describe) = describe {
        println!("cargo:rustc-env=CLOUDAGENT_BUILD_VERSION={describe}");
    }
}
