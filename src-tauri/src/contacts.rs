use rusqlite::params;
use uuid::Uuid;

use crate::db::{unix_ts_now, Db};
use crate::models::{Contact, CreateContactInput, UpdateContactInput};

pub fn list_contacts(db: &Db, search: Option<&str>) -> Result<Vec<Contact>, String> {
    db.with_conn(|conn| {
        let (sql, params_vec) = if let Some(q) = search.filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", q.trim().replace('%', "\\%").replace('_', "\\_"));
            (
                "SELECT id, name, email, phone, notes, avatar_url, created_at, updated_at
                 FROM contacts
                 WHERE name LIKE ?1 OR email LIKE ?1
                 ORDER BY lower(name)",
                vec![pattern],
            )
        } else {
            (
                "SELECT id, name, email, phone, notes, avatar_url, created_at, updated_at
                 FROM contacts
                 ORDER BY lower(name)",
                vec![],
            )
        };

        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(params_vec.iter().map(|s| s as &dyn rusqlite::types::ToSql)),
                |row| contact_from_row(row),
            )
            .map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

pub fn create_contact(db: &Db, input: &CreateContactInput) -> Result<Contact, String> {
    let id = Uuid::new_v4().to_string();
    let now = unix_ts_now();
    let contact = Contact {
        id: id.clone(),
        name: input.name.trim().to_string(),
        email: input.email.trim().to_string(),
        phone: input.phone.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        notes: input.notes.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        avatar_url: None,
        created_at: now,
        updated_at: now,
    };

    db.with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO contacts (id, name, email, phone, notes, avatar_url, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)",
            params![contact.id, contact.name, contact.email, contact.phone, contact.notes, now, now],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    Ok(contact)
}

pub fn update_contact(db: &Db, id: &str, input: &UpdateContactInput) -> Result<Contact, String> {
    let now = unix_ts_now();
    db.with_conn_mut(|conn| {
        conn.execute(
            "UPDATE contacts
             SET name = ?1, email = ?2, phone = ?3, notes = ?4, updated_at = ?5
             WHERE id = ?6",
            params![
                input.name.trim(),
                input.email.trim(),
                input.phone.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                input.notes.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                now,
                id
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;

    db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, name, email, phone, notes, avatar_url, created_at, updated_at
             FROM contacts WHERE id = ?1",
            params![id],
            |row| contact_from_row(row),
        )
        .map_err(|e| e.to_string())
    })
}

pub fn delete_contact(db: &Db, id: &str) -> Result<(), String> {
    db.with_conn_mut(|conn| {
        conn.execute("DELETE FROM contacts WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn import_contacts_from_mail(db: &Db) -> Result<usize, String> {
    let now = unix_ts_now();
    db.with_conn_mut(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT from_name, from_email
                 FROM messages
                 WHERE from_email NOT IN (SELECT email FROM contacts)
                 ORDER BY from_email",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        let mut count = 0usize;
        for (name, email) in rows {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT OR IGNORE INTO contacts (id, name, email, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, email, now, now],
            )
            .map_err(|e| e.to_string())?;
            if conn.changes() > 0 {
                count += 1;
            }
        }
        Ok(count)
    })
}

fn contact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Contact> {
    Ok(Contact {
        id: row.get(0)?,
        name: row.get(1)?,
        email: row.get(2)?,
        phone: row.get(3)?,
        notes: row.get(4)?,
        avatar_url: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
