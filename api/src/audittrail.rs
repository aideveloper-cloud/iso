// บันทึกการใช้งาน (audit trail) สำหรับผู้ตรวจสอบ/Auditor และการสำรองฐานข้อมูลอัตโนมัติ
use crate::eform::{self, AuthUser};
use crate::{err, S};
use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf};

/// GET /api/audit-log?q=&action=&actor=&from=YYYY-MM-DD&to=YYYY-MM-DD&limit= — ผู้อนุมัติขึ้นไป
pub async fn list(State(st): State<S>, Extension(u): Extension<AuthUser>, Query(q): Query<HashMap<String, String>>) -> Response {
    if !eform::can_approve(&u.role) { return err(StatusCode::FORBIDDEN, "ดูบันทึกการใช้งานได้เฉพาะผู้อนุมัติ หรือผู้ดูแลระบบ"); }
    let Ok(c) = st.db.get().await else { return err(StatusCode::INTERNAL_SERVER_ERROR, "เชื่อมต่อฐานข้อมูลไม่ได้") };
    let g = |k: &str| q.get(k).map(|v| v.trim().to_string()).unwrap_or_default();
    let needle = format!("%{}%", g("q"));
    let (action, actor, from, to) = (g("action"), g("actor"), g("from"), g("to"));
    // วันที่ตามเวลาไทย: แปลงขอบเขตเป็น UTC ISO เพื่อเทียบกับคอลัมน์ at (RFC3339 UTC)
    let bound = |d: &str, end: bool| -> String {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
            .map(|day| {
                let t = if end { day.and_hms_opt(23, 59, 59).unwrap() } else { day.and_hms_opt(0, 0, 0).unwrap() };
                (t - chrono::Duration::hours(7)).format("%Y-%m-%dT%H:%M:%S").to_string()
            }).unwrap_or_default()
    };
    let (f, t) = (bound(&from, false), bound(&to, true));
    let limit: i64 = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500).clamp(1, 5000);
    let rows = c.query(
        "SELECT id,actor_code,actor_name,action,entity,entity_id,detail,at FROM audit_log
         WHERE ($1='%%' OR actor_name ILIKE $1 OR entity_id ILIKE $1 OR detail ILIKE $1 OR action ILIKE $1)
           AND ($2='' OR action LIKE $2 || '%') AND ($3='' OR actor_code=$3)
           AND ($4='' OR at>=$4) AND ($5='' OR at<=$5)
         ORDER BY id DESC LIMIT $6",
        &[&needle, &action, &actor, &f, &t, &limit],
    ).await;
    match rows {
        Ok(rows) => Json(Value::Array(rows.iter().map(|r| json!({
            "id": r.get::<_, i64>(0), "actorCode": r.get::<_, String>(1), "actorName": r.get::<_, String>(2), "action": r.get::<_, String>(3),
            "entity": r.get::<_, String>(4), "entityId": r.get::<_, String>(5), "detail": r.get::<_, String>(6), "at": r.get::<_, String>(7),
        })).collect())).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// สำรองฐานข้อมูลทั้งหมดวันละครั้ง (pg_dump ผ่าน Docker) เก็บ 30 วันล่าสุด
/// ตั้งชื่อ container ด้วย env BACKUP_PG_CONTAINER (ค่าเริ่มต้น iso-postgres; ตั้งเป็นค่าว่างเพื่อเรียก pg_dump ตรง)
pub async fn db_backup(dir: &PathBuf) {
    let _ = std::fs::create_dir_all(dir);
    let container = std::env::var("BACKUP_PG_CONTAINER").unwrap_or_else(|_| "iso-postgres".into());
    let day = chrono::Utc::now().format("%Y%m%d").to_string();
    let target = dir.join(format!("db-{day}.sql"));
    // ชื่อฐานข้อมูล/ผู้ใช้เอาจาก DATABASE_URL ตัวเดียวกับที่ API ใช้ (key=value)
    let url = std::env::var("DATABASE_URL").unwrap_or_default();
    let kv = |k: &str, d: &str| url.split_whitespace().find_map(|p| p.strip_prefix(&format!("{k}=")).map(String::from)).unwrap_or_else(|| d.into());
    let (user, db) = (kv("user", "iso"), kv("dbname", "iso"));
    // BACKUP_PG_CONTAINER ว่าง = API รันใน container เอง → เรียก pg_dump ต่อฐานข้อมูลตรง (ใช้ host/port/password จาก DATABASE_URL)
    let out = if container.is_empty() {
        tokio::process::Command::new("pg_dump")
            .args(["-h", &kv("host", "localhost"), "-p", &kv("port", "5432"), "-U", &user, &db])
            .env("PGPASSWORD", kv("password", ""))
            .output().await
    } else {
        tokio::process::Command::new("docker")
            .args(["exec", &container, "pg_dump", "-U", &user, &db])
            .output().await
    };
    match out {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => {
            let tmp = target.with_extension("sql.part");
            if std::fs::write(&tmp, &o.stdout).is_ok() { let _ = std::fs::rename(&tmp, &target); }
            println!("  💾 สำรองฐานข้อมูล: {} ({} KB)", target.display(), o.stdout.len() / 1024);
        }
        Ok(o) => eprintln!("  ⚠ สำรองฐานข้อมูลไม่สำเร็จ: {}", String::from_utf8_lossy(&o.stderr).trim()),
        Err(e) => eprintln!("  ⚠ สำรองฐานข้อมูลไม่สำเร็จ (เรียก docker ไม่ได้): {e}"),
    }
    // ลบไฟล์สำรองอัตโนมัติที่เก่ากว่า 30 วัน (ไม่แตะไฟล์ pre-*.sql ที่สำรองด้วยมือก่อน deploy)
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).format("%Y%m%d").to_string();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let stamp = name.strip_prefix("db-").or_else(|| name.strip_prefix("forms-")).map(|r| r.chars().take(8).collect::<String>());
            if let Some(d) = stamp { if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) && d < cutoff { let _ = std::fs::remove_file(e.path()); } }
        }
    }
}
