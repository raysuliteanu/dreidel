use strum::{Display, EnumIter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumIter)]
pub enum ComponentId {
    StatusBar,
    Cpu,
    Mem,
    Net,
    Disk,
    Process,
    Debug,
}
