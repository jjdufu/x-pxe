//! 软件包分组：版本化内置数据 `data/package-groups.json`，8 个分组的并集等于 legacy `PKGS`。

use serde::{Deserialize, Serialize};

use super::types::Flag;
use crate::value::PackageName;
use crate::versioning::ConfigError;

/// 内置分组数据。
const GROUPS_JSON: &str = include_str!("../../data/package-groups.json");

/// 软件包分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageGroup {
    /// 基础运维。
    BaseOps,
    /// 网络诊断。
    NetDiag,
    /// 性能观测。
    Perf,
    /// 存储与网络栈。
    Storage,
    /// 开发构建。
    Build,
    /// 安全。
    Security,
    /// 无头浏览器运行库。
    Headless,
    /// 文档与字体。
    DocsFonts,
}

impl PackageGroup {
    /// 固定顺序的全部分组。
    pub const ALL: [Self; 8] = [
        Self::BaseOps,
        Self::NetDiag,
        Self::Perf,
        Self::Storage,
        Self::Build,
        Self::Security,
        Self::Headless,
        Self::DocsFonts,
    ];
}

/// 8 个分组开关（默认全部开启，即 `deb13-init-scripts` 取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackageGroups {
    /// 基础运维。
    pub base_ops: Flag,
    /// 网络诊断。
    pub net_diag: Flag,
    /// 性能观测。
    pub perf: Flag,
    /// 存储与网络栈。
    pub storage: Flag,
    /// 开发构建。
    pub build: Flag,
    /// 安全。
    pub security: Flag,
    /// 无头浏览器运行库。
    pub headless: Flag,
    /// 文档与字体。
    pub docs_fonts: Flag,
}

impl Default for PackageGroups {
    fn default() -> Self {
        Self {
            base_ops: Flag::ON,
            net_diag: Flag::ON,
            perf: Flag::ON,
            storage: Flag::ON,
            build: Flag::ON,
            security: Flag::ON,
            headless: Flag::ON,
            docs_fonts: Flag::ON,
        }
    }
}

impl PackageGroups {
    /// 分组是否开启。
    #[must_use]
    pub const fn is_enabled(self, group: PackageGroup) -> bool {
        match group {
            PackageGroup::BaseOps => self.base_ops.is_on(),
            PackageGroup::NetDiag => self.net_diag.is_on(),
            PackageGroup::Perf => self.perf.is_on(),
            PackageGroup::Storage => self.storage.is_on(),
            PackageGroup::Build => self.build.is_on(),
            PackageGroup::Security => self.security.is_on(),
            PackageGroup::Headless => self.headless.is_on(),
            PackageGroup::DocsFonts => self.docs_fonts.is_on(),
        }
    }
}

/// `package-groups.json` 的形状。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    /// 数据版本。
    version: u32,
    /// Debian 主版本。
    debian: String,
    /// 分组内容。
    groups: CatalogGroups,
}

/// 分组内容。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogGroups {
    /// 基础运维。
    base_ops: Vec<PackageName>,
    /// 网络诊断。
    net_diag: Vec<PackageName>,
    /// 性能观测。
    perf: Vec<PackageName>,
    /// 存储与网络栈。
    storage: Vec<PackageName>,
    /// 开发构建。
    build: Vec<PackageName>,
    /// 安全。
    security: Vec<PackageName>,
    /// 无头浏览器运行库。
    headless: Vec<PackageName>,
    /// 文档与字体。
    docs_fonts: Vec<PackageName>,
}

/// 已加载的分组清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCatalog {
    /// 数据版本。
    version: u32,
    /// 分组内容。
    groups: CatalogGroups,
}

impl PackageCatalog {
    /// 加载内置清单。
    ///
    /// # Errors
    ///
    /// 内置 JSON 损坏时返回错误。
    pub fn builtin() -> Result<Self, ConfigError> {
        let file: CatalogFile = serde_json::from_str(GROUPS_JSON)?;
        if file.debian != "13" {
            return Err(ConfigError::Json(format!(
                "分组数据面向 Debian {}",
                file.debian
            )));
        }
        Ok(Self {
            version: file.version,
            groups: file.groups,
        })
    }

    /// 数据版本。
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// 某分组的包。
    #[must_use]
    pub fn group(&self, group: PackageGroup) -> &[PackageName] {
        match group {
            PackageGroup::BaseOps => &self.groups.base_ops,
            PackageGroup::NetDiag => &self.groups.net_diag,
            PackageGroup::Perf => &self.groups.perf,
            PackageGroup::Storage => &self.groups.storage,
            PackageGroup::Build => &self.groups.build,
            PackageGroup::Security => &self.groups.security,
            PackageGroup::Headless => &self.groups.headless,
            PackageGroup::DocsFonts => &self.groups.docs_fonts,
        }
    }

    /// 全部分组的并集（按分组顺序，去重）。
    #[must_use]
    pub fn all_packages(&self) -> Vec<PackageName> {
        let mut out: Vec<PackageName> = Vec::new();
        let names = PackageGroup::ALL
            .into_iter()
            .flat_map(|group| self.group(group).iter());
        for name in names {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        out
    }
}
