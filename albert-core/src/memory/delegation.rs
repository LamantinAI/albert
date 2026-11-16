use kaeru_rig::KaeruMemory;
use rig::tool::ToolDyn;

/// Candidate constructors match the pinned canonical Kaeru surface. Children
/// receive only explicitly granted tools, after host read-only filtering.
pub(super) fn embedded_tools(memory: &KaeruMemory, clouds: bool) -> Vec<Box<dyn ToolDyn>> {
    let mut tools: Vec<Box<dyn ToolDyn>> = vec![
        Box::new(memory.context()),
        Box::new(memory.search()),
        Box::new(memory.read()),
        Box::new(memory.note()),
        Box::new(memory.reference()),
        Box::new(memory.task()),
        Box::new(memory.task_update()),
        Box::new(memory.claim()),
        Box::new(memory.verdict()),
        Box::new(memory.revise()),
        Box::new(memory.replace()),
        Box::new(memory.relate()),
        Box::new(memory.review()),
        Box::new(memory.trail()),
        Box::new(memory.pin()),
        Box::new(memory.reflect()),
        Box::new(memory.lifecycle()),
        Box::new(memory.synthesise()),
        Box::new(memory.forget()),
        Box::new(memory.history()),
        Box::new(memory.retention()),
        Box::new(memory.initiatives()),
        Box::new(memory.initiative_manage()),
        Box::new(memory.board_config()),
        Box::new(memory.policy()),
        Box::new(memory.export()),
    ];
    if clouds {
        tools.extend(vec![
            Box::new(memory.clouds()) as Box<dyn ToolDyn>,
            Box::new(memory.share()),
            Box::new(memory.unshare()),
            Box::new(memory.cloud_initiatives()),
            Box::new(memory.cloud_recall()),
            Box::new(memory.pull()),
            Box::new(memory.link_cloud()),
            Box::new(memory.cloud_links()),
            Box::new(memory.sync_review()),
        ]);
    }
    tools
}
