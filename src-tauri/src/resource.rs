use std::collections::HashMap;

use reqwest::Client;
use serde::{Deserialize, Serialize};

const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const CURSEFORGE_API: &str = "https://api.curseforge.com/v1";
const MINECRAFT_GAME_ID: &str = "432";
const CURSEFORGE_SHADER_CLASS_ID: &str = "6552";

/// Mirrors PCL2's `DlSourceModGet` (Modules/Minecraft/ModDownload.vb) so the
/// resource center talks to the same download source as Plain Craft Launcher.
fn mci_mirror_url(url: &str) -> String {
    url.replace("api.modrinth.com", "mod.mcimirror.top/modrinth")
        .replace("staging-api.modrinth.com", "mod.mcimirror.top/modrinth")
        .replace("cdn.modrinth.com", "mod.mcimirror.top")
        .replace("api.curseforge.com", "mod.mcimirror.top/curseforge")
        .replace("edge.forgecdn.net", "mod.mcimirror.top")
        .replace("mediafilez.forgecdn.net", "mod.mcimirror.top")
        .replace("media.forgecdn.net", "mod.mcimirror.top")
}

/// Mirror first, then the official endpoint. PCL2 defaults to the mirror and
/// only falls back to the original host, so requests keep working either way.
fn curseforge_api_sources() -> Vec<(String, Option<String>)> {
    let mut sources = vec![(mci_mirror_url(CURSEFORGE_API), None)];
    if let Ok(key) = curseforge_api_key() {
        sources.push((CURSEFORGE_API.to_string(), Some(key)));
    }
    sources
}

fn modrinth_api_sources() -> Vec<String> {
    vec![mci_mirror_url(MODRINTH_API), MODRINTH_API.to_string()]
}

/// The search URL already points at the official API, so only swap the host.
fn modrinth_search_sources(official_url: &str) -> Vec<String> {
    vec![mci_mirror_url(official_url), official_url.to_string()]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSearchQuery {
    pub(crate) resource_type: String,
    pub(crate) search_query: String,
    pub(crate) game_version: String,
    pub(crate) selected_tag: String,
    pub(crate) sort_by: String,
    pub(crate) download_source: String,
    pub(crate) page: u32,
    pub(crate) page_size: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceItem {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) icon_url: String,
    /// 镜像图标加载失败时使用的官方地址；没有镜像差异时为 None。
    pub(crate) icon_fallback_url: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) last_updated: String,
    pub(crate) downloads: u64,
    pub(crate) source: String,
    pub(crate) website_url: String,
    pub(crate) author: String,
    pub(crate) resource_type: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSearchResponse {
    pub(crate) items: Vec<ResourceItem>,
    pub(crate) total: u64,
    pub(crate) page: u32,
    pub(crate) page_size: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceFile {
    pub(crate) resource_id: String,
    pub(crate) name: String,
    pub(crate) release_type: String,
    pub(crate) downloads: u64,
    pub(crate) file_date: String,
    pub(crate) download_url: String,
    /// 主地址失败后依次尝试的备用地址（官方源、CDN 域名变体等）。
    pub(crate) fallback_download_urls: Vec<String>,
    pub(crate) file_name: String,
    pub(crate) game_versions: Vec<String>,
    pub(crate) loaders: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceVersionPack {
    pub(crate) name: String,
    pub(crate) items: Vec<ResourceFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSourceOption {
    pub(crate) value: String,
    pub(crate) label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceOptions {
    pub(crate) sources: Vec<ResourceSourceOption>,
    pub(crate) tags: HashMap<String, Vec<String>>,
    pub(crate) sort_options: HashMap<String, Vec<ResourceSourceOption>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MinecraftVersionOption {
    pub(crate) value: String,
    pub(crate) label: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersionManifest {
    versions: Vec<MinecraftVersion>,
}

#[derive(Debug, Deserialize)]
struct MinecraftVersion {
    id: String,
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthProject>,
    total_hits: u64,
    offset: u32,
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    project_id: String,
    project_type: String,
    slug: String,
    title: String,
    description: String,
    categories: Vec<String>,
    downloads: u64,
    icon_url: Option<String>,
    date_modified: String,
    author: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    name: String,
    date_published: String,
    downloads: u64,
    version_type: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<ModrinthFile>,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    url: String,
    filename: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeSearchResponse {
    data: Vec<CurseForgeProject>,
    pagination: CurseForgePagination,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeProject {
    id: u32,
    links: CurseForgeLinks,
    name: String,
    summary: String,
    categories: Vec<CurseForgeCategory>,
    download_count: u64,
    logo: Option<CurseForgeLogo>,
    date_modified: String,
    authors: Vec<CurseForgeAuthor>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeLinks {
    website_url: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeCategory {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeLogo {
    url: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgePagination {
    index: u32,
    page_size: u32,
    total_count: u64,
}

#[derive(Debug, Deserialize)]
struct CurseForgeFileResponse {
    data: Vec<CurseForgeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeFile {
    id: u32,
    mod_id: u32,
    display_name: String,
    file_name: String,
    release_type: u32,
    download_url: Option<String>,
    download_count: u64,
    game_versions: Vec<String>,
    file_date: String,
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn append_query_pair(query: &mut String, key: &str, value: &str) {
    if !query.is_empty() {
        query.push('&');
    }
    query.push_str(&encode_query_component(key));
    query.push('=');
    query.push_str(&encode_query_component(value));
}

pub(crate) fn build_modrinth_search_url(
    search_query: &str,
    game_version: &str,
    selected_tag: &str,
    sort_by: &str,
    page: u32,
    page_size: u32,
) -> String {
    let mut facets = vec![vec!["project_type:shader".to_string()]];
    if !game_version.is_empty() && game_version != "All" {
        facets.push(vec![format!("versions:{game_version}")]);
    }
    if !selected_tag.is_empty() && selected_tag != "All" {
        facets.push(vec![format!("categories:{selected_tag}")]);
    }

    let mut query = String::new();
    append_query_pair(&mut query, "query", search_query);
    append_query_pair(
        &mut query,
        "facets",
        &serde_json::to_string(&facets).unwrap_or_default(),
    );
    append_query_pair(
        &mut query,
        "offset",
        &((page.saturating_sub(1)) * page_size).to_string(),
    );
    append_query_pair(&mut query, "limit", &page_size.to_string());
    append_query_pair(&mut query, "index", sort_by);
    format!("{MODRINTH_API}/search?{query}")
}

fn map_modrinth_search_response(response: ModrinthSearchResponse) -> ResourceSearchResponse {
    ResourceSearchResponse {
        items: response
            .hits
            .into_iter()
            .map(|project| ResourceItem {
                id: project.project_id,
                name: project.title,
                description: project.description,
                icon_fallback_url: project
                    .icon_url
                    .clone()
                    .filter(|url| mci_mirror_url(url) != *url),
                icon_url: project
                    .icon_url
                    .map(|url| mci_mirror_url(&url))
                    .unwrap_or_default(),
                tags: project.categories,
                last_updated: project.date_modified,
                downloads: project.downloads,
                source: "Modrinth".to_string(),
                website_url: format!("https://modrinth.com/shader/{}", project.slug),
                author: project.author.unwrap_or_default(),
                resource_type: project.project_type,
            })
            .collect(),
        total: response.total_hits,
        page: response.offset / response.limit.max(1) + 1,
        page_size: response.limit,
    }
}

fn curseforge_api_key() -> Result<String, String> {
    std::env::var("SJMCL_CURSEFORGE_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未配置 SJMCL_CURSEFORGE_API_KEY".to_string())
}

fn curseforge_sort_field(sort_by: &str) -> u32 {
    match sort_by {
        "A-Z" => 4,
        "Latest update" => 3,
        "Creation date" => 11,
        "Total downloads" => 6,
        _ => 2,
    }
}

fn curseforge_sort_order(sort_by: &str) -> &'static str {
    if sort_by == "A-Z" {
        "asc"
    } else {
        "desc"
    }
}

fn curseforge_tag_id(tag: &str) -> u32 {
    match tag {
        "Vanilla" => 6555,
        "Fantasy" => 6554,
        "Realistic" => 6553,
        _ => 0,
    }
}

fn release_type(value: u32) -> String {
    match value {
        1 => "release",
        2 => "beta",
        _ => "alpha",
    }
    .to_string()
}

async fn fetch_modrinth_resources(
    query: &ResourceSearchQuery,
) -> Result<ResourceSearchResponse, String> {
    let url = build_modrinth_search_url(
        &query.search_query,
        &query.game_version,
        &query.selected_tag,
        &query.sort_by,
        query.page,
        query.page_size,
    );
    let mut response = None;
    let mut errors = Vec::new();
    for candidate in modrinth_search_sources(&url) {
        match Client::new()
            .get(&candidate)
            .header("User-Agent", "ProjectZomboidManager/0.1")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(ok) => match ok.json::<ModrinthSearchResponse>().await {
                Ok(parsed) => {
                    response = Some(parsed);
                    break;
                }
                Err(error) => errors.push(format!("解析 Modrinth 结果失败: {error}")),
            },
            Err(error) => errors.push(format!("请求 Modrinth 失败: {error}")),
        }
    }
    let response = response.ok_or_else(|| errors.join("; "))?;
    Ok(map_modrinth_search_response(response))
}

async fn fetch_curseforge_resources(
    query: &ResourceSearchQuery,
) -> Result<ResourceSearchResponse, String> {
    let mut response = None;
    let mut errors = Vec::new();
    for (base, api_key) in curseforge_api_sources() {
        let mut request = Client::new().get(format!("{base}/mods/search")).query(&[
            ("gameId", MINECRAFT_GAME_ID.to_string()),
            ("classId", CURSEFORGE_SHADER_CLASS_ID.to_string()),
            ("searchFilter", query.search_query.clone()),
            (
                "sortField",
                curseforge_sort_field(&query.sort_by).to_string(),
            ),
            (
                "sortOrder",
                curseforge_sort_order(&query.sort_by).to_string(),
            ),
            (
                "index",
                ((query.page.saturating_sub(1)) * query.page_size).to_string(),
            ),
            ("pageSize", query.page_size.to_string()),
        ]);
        if let Some(api_key) = api_key {
            request = request.header("x-api-key", api_key);
        }
        if query.game_version != "All" && !query.game_version.is_empty() {
            request = request.query(&[("gameVersion", query.game_version.clone())]);
        }
        if query.selected_tag != "All" && !query.selected_tag.is_empty() {
            request = request.query(&[(
                "categoryId",
                curseforge_tag_id(&query.selected_tag).to_string(),
            )]);
        }
        match request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(ok) => match ok.json::<CurseForgeSearchResponse>().await {
                Ok(parsed) => {
                    response = Some(parsed);
                    break;
                }
                Err(error) => errors.push(format!("解析 CurseForge 结果失败: {error}")),
            },
            Err(error) => errors.push(format!("请求 CurseForge 失败 ({base}): {error}")),
        }
    }
    let response = response.ok_or_else(|| errors.join("; "))?;

    Ok(ResourceSearchResponse {
        items: response
            .data
            .into_iter()
            .map(|project| ResourceItem {
                id: project.id.to_string(),
                name: project.name,
                description: project.summary,
                icon_url: project
                    .logo
                    .as_ref()
                    .map(|logo| mci_mirror_url(&logo.url))
                    .unwrap_or_default(),
                icon_fallback_url: project
                    .logo
                    .map(|logo| logo.url)
                    .filter(|url| mci_mirror_url(url) != *url),
                tags: project
                    .categories
                    .into_iter()
                    .map(|category| category.name)
                    .collect(),
                last_updated: project.date_modified,
                downloads: project.download_count,
                source: "CurseForge".to_string(),
                website_url: project.links.website_url,
                author: project
                    .authors
                    .first()
                    .map(|author| author.name.clone())
                    .unwrap_or_default(),
                resource_type: "shader".to_string(),
            })
            .collect(),
        total: response.pagination.total_count,
        page: response.pagination.index / response.pagination.page_size.max(1) + 1,
        page_size: response.pagination.page_size,
    })
}

async fn fetch_modrinth_files(resource_id: &str) -> Result<Vec<ResourceVersionPack>, String> {
    let mut versions = None;
    let mut errors = Vec::new();
    for base in modrinth_api_sources() {
        match Client::new()
            .get(format!("{base}/project/{resource_id}/version"))
            .header("User-Agent", "ProjectZomboidManager/0.1")
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(ok) => match ok.json::<Vec<ModrinthVersion>>().await {
                Ok(parsed) => {
                    versions = Some(parsed);
                    break;
                }
                Err(error) => errors.push(format!("解析 Modrinth 光影版本失败: {error}")),
            },
            Err(error) => errors.push(format!("请求 Modrinth 光影版本失败: {error}")),
        }
    }
    let versions = versions.ok_or_else(|| errors.join("; "))?;

    Ok(versions
        .into_iter()
        .map(|version| ResourceVersionPack {
            name: version.name.clone(),
            items: version
                .files
                .into_iter()
                .map(|file| ResourceFile {
                    resource_id: resource_id.to_string(),
                    name: version.name.clone(),
                    release_type: version.version_type.clone(),
                    downloads: version.downloads,
                    file_date: version.date_published.clone(),
                    // PCL2 downloads Modrinth files from the CDN mirror.
                    download_url: mci_mirror_url(&file.url),
                    fallback_download_urls: vec![file.url],
                    file_name: file.filename,
                    game_versions: version.game_versions.clone(),
                    loaders: version.loaders.clone(),
                })
                .collect(),
        })
        .filter(|pack| !pack.items.is_empty())
        .collect())
}

async fn fetch_curseforge_files(resource_id: &str) -> Result<Vec<ResourceVersionPack>, String> {
    let mut response = None;
    let mut errors = Vec::new();
    for (base, api_key) in curseforge_api_sources() {
        let mut request = Client::new()
            .get(format!("{base}/mods/{resource_id}/files"))
            .query(&[("pageSize", "50".to_string())]);
        if let Some(api_key) = api_key {
            request = request.header("x-api-key", api_key);
        }
        match request
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(ok) => match ok.json::<CurseForgeFileResponse>().await {
                Ok(parsed) => {
                    response = Some(parsed);
                    break;
                }
                Err(error) => errors.push(format!("解析 CurseForge 光影版本失败: {error}")),
            },
            Err(error) => errors.push(format!("请求 CurseForge 光影版本失败 ({base}): {error}")),
        }
    }
    let response = response.ok_or_else(|| errors.join("; "))?;

    Ok(response
        .data
        .into_iter()
        .map(|file| {
            // 官方 API 没给 downloadUrl 时，和 PCL2 一样按文件 ID 拼 CDN 地址；
            // 旧逻辑这里退回的是网页地址，下载下来的会是 HTML 而不是光影包。
            let official_download = file
                .download_url
                .filter(|url| !url.trim().is_empty())
                .unwrap_or_else(|| curseforge_cdn_url(file.id, &file.file_name));
            let download_url = mci_mirror_url(&official_download);
            let fallback_download_urls =
                curseforge_fallback_urls(&official_download, &download_url);
            ResourceVersionPack {
                name: file.display_name.clone(),
                items: vec![ResourceFile {
                    resource_id: file.mod_id.to_string(),
                    name: file.display_name,
                    release_type: release_type(file.release_type),
                    downloads: file.download_count,
                    file_date: file.file_date,
                    download_url,
                    fallback_download_urls,
                    file_name: file.file_name,
                    game_versions: file.game_versions,
                    loaders: Vec::new(),
                }],
            }
        })
        .collect())
}

/// 按文件 ID 拼 CurseForge CDN 地址，切分方式和 PCL2 的 `Url.Substring` 一致。
fn curseforge_cdn_url(file_id: u32, file_name: &str) -> String {
    let digits = file_id.to_string();
    let (head, tail) = digits.split_at(digits.len().min(4));
    format!(
        "https://edge.forgecdn.net/files/{}/{}/{}",
        head,
        tail.parse::<u32>().unwrap_or(0),
        encode_query_component(file_name)
    )
}

/// 官方 CDN 的几个域名互为备用（edge / media / mediafilez / overwolf），
/// 与 PCL2 的 `ParseCurseForgeDownloadUrls` 候选顺序一致；镜像失败后依次尝试，
/// 去掉与主地址重复的项。
fn curseforge_fallback_urls(official: &str, primary: &str) -> Vec<String> {
    let variants = [
        official
            .replace("-service.overwolf.wtf", ".forgecdn.net")
            .replace("://edge.", "://mediafilez.")
            .replace("://media.", "://mediafilez."),
        official
            .replace("://edge.", "://mediafilez.")
            .replace("://media.", "://mediafilez."),
        official.replace("-service.overwolf.wtf", ".forgecdn.net"),
        official.replace("://media.", "://edge."),
        official.to_string(),
    ];
    let mut urls: Vec<String> = Vec::new();
    for url in variants {
        if url != primary && !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

pub(crate) fn resource_options() -> ResourceOptions {
    ResourceOptions {
        sources: vec![
            ResourceSourceOption {
                value: "CurseForge".to_string(),
                label: "CurseForge".to_string(),
            },
            ResourceSourceOption {
                value: "Modrinth".to_string(),
                label: "Modrinth".to_string(),
            },
        ],
        tags: HashMap::from([
            (
                "CurseForge".to_string(),
                vec![
                    "All".to_string(),
                    "Fantasy".to_string(),
                    "Realistic".to_string(),
                    "Vanilla".to_string(),
                ],
            ),
            (
                "Modrinth".to_string(),
                vec![
                    "All".to_string(),
                    "cartoon".to_string(),
                    "cursed".to_string(),
                    "fantasy".to_string(),
                    "realistic".to_string(),
                    "semi-realistic".to_string(),
                    "vanilla-like".to_string(),
                    "atmosphere".to_string(),
                    "bloom".to_string(),
                    "colored-lighting".to_string(),
                    "foliage".to_string(),
                    "path-tracing".to_string(),
                    "pbr".to_string(),
                    "reflections".to_string(),
                    "shadows".to_string(),
                    "potato".to_string(),
                    "low".to_string(),
                    "medium".to_string(),
                    "high".to_string(),
                    "screenshot".to_string(),
                ],
            ),
        ]),
        sort_options: HashMap::from([
            (
                "CurseForge".to_string(),
                vec![
                    ResourceSourceOption {
                        value: "Popularity".to_string(),
                        label: "热度".to_string(),
                    },
                    ResourceSourceOption {
                        value: "A-Z".to_string(),
                        label: "名称".to_string(),
                    },
                    ResourceSourceOption {
                        value: "Latest update".to_string(),
                        label: "最近更新".to_string(),
                    },
                    ResourceSourceOption {
                        value: "Creation date".to_string(),
                        label: "创建时间".to_string(),
                    },
                    ResourceSourceOption {
                        value: "Total downloads".to_string(),
                        label: "下载量".to_string(),
                    },
                ],
            ),
            (
                "Modrinth".to_string(),
                vec![
                    ResourceSourceOption {
                        value: "relevance".to_string(),
                        label: "相关性".to_string(),
                    },
                    ResourceSourceOption {
                        value: "downloads".to_string(),
                        label: "下载量".to_string(),
                    },
                    ResourceSourceOption {
                        value: "follows".to_string(),
                        label: "关注量".to_string(),
                    },
                    ResourceSourceOption {
                        value: "updated".to_string(),
                        label: "最近更新".to_string(),
                    },
                    ResourceSourceOption {
                        value: "newest".to_string(),
                        label: "最新发布".to_string(),
                    },
                ],
            ),
        ]),
    }
}

pub(crate) async fn fetch_resources(
    query: ResourceSearchQuery,
) -> Result<ResourceSearchResponse, String> {
    if query.resource_type != "shader" {
        return Err("资源中心当前仅支持 Minecraft 光影包".to_string());
    }
    match query.download_source.as_str() {
        "CurseForge" => fetch_curseforge_resources(&query).await,
        "Modrinth" => fetch_modrinth_resources(&query).await,
        _ => Err("未知的 Minecraft 光影源".to_string()),
    }
}

pub(crate) async fn fetch_resource_files(
    resource_id: String,
    download_source: String,
) -> Result<Vec<ResourceVersionPack>, String> {
    match download_source.as_str() {
        "CurseForge" => fetch_curseforge_files(&resource_id).await,
        "Modrinth" => fetch_modrinth_files(&resource_id).await,
        _ => Err("未知的 Minecraft 光影源".to_string()),
    }
}

pub(crate) async fn fetch_minecraft_versions() -> Result<Vec<MinecraftVersionOption>, String> {
    let response = Client::new()
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .header("User-Agent", "ProjectZomboidManager/0.1")
        .send()
        .await
        .map_err(|error| format!("请求 Minecraft 版本列表失败: {error}"))?
        .error_for_status()
        .map_err(|error| format!("请求 Minecraft 版本列表失败: {error}"))?
        .json::<MinecraftVersionManifest>()
        .await
        .map_err(|error| format!("解析 Minecraft 版本列表失败: {error}"))?;

    Ok(std::iter::once(MinecraftVersionOption {
        value: "All".to_string(),
        label: "全部".to_string(),
    })
    .chain(
        response
            .versions
            .into_iter()
            .filter(|version| version.r#type == "release")
            .map(|version| MinecraftVersionOption {
                value: version.id.clone(),
                label: version.id,
            }),
    )
    .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        build_modrinth_search_url, curseforge_api_sources, curseforge_cdn_url,
        curseforge_fallback_urls, map_modrinth_search_response, mci_mirror_url,
        modrinth_api_sources, ModrinthProject, ModrinthSearchResponse,
    };

    #[test]
    fn maps_pcl2_mod_mirror_urls() {
        assert_eq!(
            mci_mirror_url("https://api.curseforge.com/v1/mods/search"),
            "https://mod.mcimirror.top/curseforge/v1/mods/search"
        );
        assert_eq!(
            mci_mirror_url("https://edge.forgecdn.net/files/1/2/a.zip"),
            "https://mod.mcimirror.top/files/1/2/a.zip"
        );
        assert_eq!(
            mci_mirror_url("https://mediafilez.forgecdn.net/files/1/2/a.zip"),
            "https://mod.mcimirror.top/files/1/2/a.zip"
        );
        assert_eq!(
            mci_mirror_url("https://api.modrinth.com/v2/search"),
            "https://mod.mcimirror.top/modrinth/v2/search"
        );
        assert_eq!(
            mci_mirror_url("https://cdn.modrinth.com/data/x/y.zip"),
            "https://mod.mcimirror.top/data/x/y.zip"
        );
    }

    #[test]
    fn tries_the_mirror_before_the_official_api() {
        let sources = curseforge_api_sources();
        assert!(sources[0]
            .0
            .starts_with("https://mod.mcimirror.top/curseforge"));
        assert!(sources[0].1.is_none());

        let modrinth = modrinth_api_sources();
        assert_eq!(modrinth[0], "https://mod.mcimirror.top/modrinth/v2");
        assert_eq!(modrinth[1], "https://api.modrinth.com/v2");
    }

    #[test]
    fn builds_sjmcl_compatible_shader_search_url() {
        let url =
            build_modrinth_search_url("Complementary", "1.21.1", "realistic", "downloads", 2, 20);

        assert!(url.starts_with("https://api.modrinth.com/v2/search?"));
        assert!(url.contains("query=Complementary"));
        assert!(url.contains("offset=20"));
        assert!(url.contains("limit=20"));
        assert!(url.contains("index=downloads"));
        assert!(url.contains("project_type%3Ashader"));
        assert!(url.contains("versions%3A1.21.1"));
        assert!(url.contains("categories%3Arealistic"));
    }

    #[test]
    fn builds_curseforge_cdn_url_like_pcl2() {
        assert_eq!(
            curseforge_cdn_url(4567890, "Complementary Shaders v5.zip"),
            "https://edge.forgecdn.net/files/4567/890/Complementary%20Shaders%20v5.zip"
        );
        assert_eq!(
            curseforge_cdn_url(1234, "a.zip"),
            "https://edge.forgecdn.net/files/1234/0/a.zip"
        );
    }

    #[test]
    fn lists_curseforge_cdn_variants_without_duplicates() {
        let primary = "https://mod.mcimirror.top/files/1/2/a.zip";
        let official = "https://edge.forgecdn.net/files/1/2/a.zip";
        let fallbacks = curseforge_fallback_urls(official, primary);

        assert_eq!(
            fallbacks,
            vec![
                "https://mediafilez.forgecdn.net/files/1/2/a.zip".to_string(),
                "https://edge.forgecdn.net/files/1/2/a.zip".to_string(),
            ]
        );
    }

    #[test]
    fn overwolf_service_urls_map_to_forgecdn() {
        let official = "https://edge-service.overwolf.wtf/files/1/2/a.zip";
        let fallbacks = curseforge_fallback_urls(official, official);
        assert!(fallbacks.contains(&"https://edge.forgecdn.net/files/1/2/a.zip".to_string()));
    }

    #[test]
    fn icon_fallback_is_none_when_mirror_is_identity() {
        let project: ModrinthProject = serde_json::from_value(serde_json::json!({
            "project_id": "abc",
            "project_type": "shader",
            "slug": "x",
            "title": "X",
            "description": "",
            "categories": [],
            "downloads": 0,
            "icon_url": "https://mod.mcimirror.top/already/mirror.png",
            "date_modified": "2026-10-01T00:00:00Z"
        }))
        .unwrap();
        let response = ModrinthSearchResponse {
            hits: vec![project],
            total_hits: 1,
            offset: 0,
            limit: 20,
        };

        let mapped = map_modrinth_search_response(response);

        assert_eq!(mapped.items[0].icon_fallback_url, None);
    }

    #[test]
    fn maps_modrinth_shader_search_items() {
        let project: ModrinthProject = serde_json::from_value(serde_json::json!({
            "project_id": "abc",
            "project_type": "shader",
            "slug": "complementary",
            "title": "Complementary",
            "description": "A shader pack",
            "categories": ["realistic", "high"],
            "downloads": 1234,
            "icon_url": "https://cdn.example/icon.png",
            "date_modified": "2026-10-01T00:00:00Z",
            "author": "作者"
        }))
        .unwrap();
        let response = ModrinthSearchResponse {
            hits: vec![project],
            total_hits: 1,
            offset: 0,
            limit: 20,
        };

        let mapped = map_modrinth_search_response(response);

        assert_eq!(mapped.total, 1);
        assert_eq!(mapped.items[0].id, "abc");
        assert_eq!(mapped.items[0].source, "Modrinth");
        assert_eq!(mapped.items[0].resource_type, "shader");
    }
}
