use yutu_core::registry::{self, Effect};

#[test]
fn registry_has_go_command_and_tool_counts() {
    assert_eq!(registry::COMMANDS.len(), 117);
    assert_eq!(registry::tools().count(), 74);
    let gated = registry::tools().filter(|(_, t)| t.gated()).count();
    assert_eq!(gated, 52);
    assert_eq!(registry::tools().filter(|(_, t)| t.read_only()).count(), 22);
    assert_eq!(registry::tools().filter(|(_, t)| t.destructive()).count(), 15);
}

#[test]
fn caption_download_writes_a_local_file() {
    let c = registry::find("caption download").unwrap();
    assert_eq!(c.tool.as_ref().unwrap().effect, Effect::WritesLocalFile);
}

#[test]
fn root_children_are_sorted_like_cobra() {
    let names: Vec<_> = registry::root().children.iter().map(|&i| registry::COMMANDS[i].name()).collect();
    assert_eq!(names.len(), 35);
    assert_eq!(&names[..3], &["abuseReport", "activity", "agent"]);
}
