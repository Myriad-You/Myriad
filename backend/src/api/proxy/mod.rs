//! HTTP proxies: image, music lyrics/audio, geo, hitokoto, and web content fetch.
//!
//! Real submodules (not `include!`) so each file owns its imports and visibility.

mod image_music_geo;
mod hitokoto_web;

pub use hitokoto_web::*;
pub use image_music_geo::*;

#[cfg(test)]
mod unbounded_read_tests {
    /// 代理模块不得存在无界的上游响应读取。
    ///
    /// `resp.bytes()` / `resp.text()` / `resp.json()` 都会把整个响应缓冲进内存。
    /// 这些端点连的是上游 CDN 与第三方 API：对方被劫持、故障，或单纯返回了
    /// 一个超大响应，都会变成本进程的内存放大 —— 而这里有**公开未认证**的
    /// 图片代理。
    ///
    /// 之前的写法是先 `bytes()` 再判断长度，代码注释自己承认
    /// "body is already buffered"：上限只能事后拒绝，拦不住内存消耗。
    ///
    /// 类型系统区分不了「这次读取有上限」，所以对源码断言。
    #[test]
    fn proxy_never_buffers_an_upstream_body_without_a_cap() {
        let src = concat!(
            include_str!("image_music_geo.rs"),
            include_str!("hitokoto_web.rs")
        );
        let code: String = src
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(src)
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        for pattern in [".bytes().await", ".text().await", ".json::<"] {
            assert!(
                !code.contains(pattern),
                "unbounded `{pattern}` in proxy — use \
                 outbound_security::read_limited_body (or read_limited_json) so the \
                 cap actually bounds memory instead of rejecting after the fact"
            );
        }
    }
}
