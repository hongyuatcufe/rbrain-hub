use regex::Regex;

/// Publication metadata extracted from either a CNKI export or an LLM auto-extract.
#[derive(Debug, Clone)]
pub struct PubMetadata {
    pub title: String,
    pub authors: Vec<String>,
    pub journal: String,
    pub year: Option<String>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    pub doi: Option<String>,
    pub organ: Option<String>,
    /// "high" (CNKI export) or "medium" (LLM auto-extract).
    pub confidence: String,
    /// Slug of the source note page this metadata was derived from, if known.
    pub source_slug: Option<String>,
}

impl PubMetadata {
    /// Canonical name used as the SaveMulti `name` field and the basis for the output slug.
    /// Format: "{title_first40}_{first_author}"
    pub fn canonical_name(&self) -> String {
        let title_part: String = self.title.chars().take(40).collect();
        let author_part = self.authors.first().cloned().unwrap_or_default();
        if author_part.is_empty() {
            title_part
        } else {
            format!("{}_{}", title_part, author_part)
        }
    }

    /// Serialize all fields as a JSON string for storage in `compiled_truth`.
    pub fn to_json_description(&self) -> String {
        let v = serde_json::json!({
            "pub_title": self.title,
            "pub_authors": self.authors,
            "pub_year": self.year,
            "pub_journal": self.journal,
            "pub_volume": self.volume,
            "pub_issue": self.issue,
            "pub_pages": self.pages,
            "pub_doi": self.doi,
            "pub_organ": self.organ,
            "confidence": self.confidence,
        });
        v.to_string()
    }
}

/// Parses CNKI export text (multiple records separated by blank lines) into `PubMetadata`.
///
/// Expected CNKI format per record:
/// ```text
/// DataType: 1
/// Title-题名: 论文标题
/// Author-作者: 张三; 李四
/// Source-刊名: 教育研究
/// Year-年: 2023
/// PubTime-出版时间: 2023-06-01
/// Period-期: 6
/// Roll-卷: 44
/// Page-页码: 23-45
/// Link-链接: https://link.cnki.net/doi/10.xxx
/// Organ-机构: 某大学
/// ```
pub struct CnkiRefParser;

impl CnkiRefParser {
    /// Parse CNKI export content and return one `PubMetadata` per non-empty record.
    pub fn parse(content: &str) -> Vec<PubMetadata> {
        // Field pattern: EnglishKey-ChineseKey: value  (the Chinese key part may be absent)
        // We match: one or more ASCII letters, optional "-anything except colon", colon, space, value
        let field_re = Regex::new(r"(?m)^([A-Za-z]+)(?:-[^:]+)?:\s*(.*)$").unwrap();

        // Split records by blank lines (handles \r\n and trailing spaces on blank lines)
        let record_re = Regex::new(r"\n[ \t]*\r?\n").unwrap();

        record_re
            .split(content)
            .filter_map(|block| {
                let block = block.trim();
                if block.is_empty() {
                    return None;
                }

                let mut title = String::new();
                let mut authors: Vec<String> = Vec::new();
                let mut journal = String::new();
                let mut year: Option<String> = None;
                let mut volume: Option<String> = None;
                let mut issue: Option<String> = None;
                let mut pages: Option<String> = None;
                let mut doi: Option<String> = None;
                let mut organ: Option<String> = None;

                for cap in field_re.captures_iter(block) {
                    let key = cap[1].to_ascii_uppercase();
                    let value = cap[2].trim().to_string();
                    if value.is_empty() {
                        continue;
                    }
                    match key.as_str() {
                        "TITLE" => title = value,
                        "AUTHOR" => {
                            authors = value
                                .split(';')
                                .map(|a| a.trim().to_string())
                                .filter(|a| !a.is_empty())
                                .collect();
                        }
                        "SOURCE" => journal = value,
                        "YEAR" => year = Some(value),
                        // PubTime-出版时间 looks like "2023-06-01"; extract year as fallback
                        "PUBTIME" => {
                            if year.is_none() {
                                year = value.split('-').next().map(|s| s.to_string());
                            }
                        }
                        "ROLL" => volume = Some(value),
                        "PERIOD" => issue = Some(value),
                        "PAGE" => pages = Some(value),
                        "LINK" => {
                            // Extract DOI from CNKI link: https://link.cnki.net/doi/10.xxx
                            doi = extract_doi_from_link(&value).or(Some(value));
                        }
                        "ORGAN" => organ = Some(value),
                        _ => {}
                    }
                }

                // Require at least a title to produce a record
                if title.is_empty() {
                    return None;
                }

                Some(PubMetadata {
                    title,
                    authors,
                    journal,
                    year,
                    volume,
                    issue,
                    pages,
                    doi,
                    organ,
                    confidence: "high".to_string(),
                    source_slug: None,
                })
            })
            .collect()
    }
}

fn extract_doi_from_link(link: &str) -> Option<String> {
    // https://link.cnki.net/doi/10.xxxxx  or  https://doi.org/10.xxxxx
    if let Some(pos) = link.find("10.") {
        Some(link[pos..].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"DataType: 1
Title-题名: 多学科视角下的高等教育学
Author-作者: 赵祥辉; 吴英琪
Source-刊名: 重庆高教研究
Year-年: 2025
PubTime-出版时间: 2025-03-01
Period-期: 2
Roll-卷: 13
Page-页码: 3-15
Link-链接: https://link.cnki.net/doi/10.15998/j.cnki.issn1673-8012.2025.02.001
Organ-机构: 东北师范大学

DataType: 1
Title-题名: 论中国教育学自主知识体系建设
Author-作者: 刘贵华; 王洁
Source-刊名: 教育研究
Year-年: 2022
PubTime-出版时间: 2022-12-01
Period-期: 12
Roll-卷: 43
Page-页码: 4-18
Link-链接: https://link.cnki.net/doi/10.3969/j.issn.0256-2928.2022.12.001
Organ-机构: 华中师范大学"#;

    #[test]
    fn parses_two_records() {
        let metas = CnkiRefParser::parse(SAMPLE);
        assert_eq!(metas.len(), 2);

        let m0 = &metas[0];
        assert_eq!(m0.title, "多学科视角下的高等教育学");
        assert_eq!(m0.authors, vec!["赵祥辉", "吴英琪"]);
        assert_eq!(m0.journal, "重庆高教研究");
        assert_eq!(m0.year.as_deref(), Some("2025"));
        assert_eq!(m0.volume.as_deref(), Some("13"));
        assert_eq!(m0.issue.as_deref(), Some("2"));
        assert_eq!(m0.confidence, "high");

        let m1 = &metas[1];
        assert_eq!(m1.title, "论中国教育学自主知识体系建设");
        assert_eq!(m1.authors[0], "刘贵华");
        assert_eq!(m1.year.as_deref(), Some("2022"));
    }

    #[test]
    fn canonical_name_uses_title_and_first_author() {
        let metas = CnkiRefParser::parse(SAMPLE);
        assert_eq!(
            metas[0].canonical_name(),
            "多学科视角下的高等教育学_赵祥辉"
        );
    }

    #[test]
    fn doi_extracted_from_link() {
        let metas = CnkiRefParser::parse(SAMPLE);
        assert!(metas[0]
            .doi
            .as_deref()
            .unwrap_or("")
            .starts_with("10.15998"));
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(CnkiRefParser::parse("").is_empty());
    }

    #[test]
    fn skips_records_without_title() {
        let bad = "Author-作者: 张三\nSource-刊名: 某刊\nYear-年: 2020\n";
        assert!(CnkiRefParser::parse(bad).is_empty());
    }
}
