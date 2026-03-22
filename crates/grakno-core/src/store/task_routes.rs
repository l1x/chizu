use crate::error::Result;
use crate::model::TaskRoute;
use crate::store::Store;

impl Store {
    pub fn insert_task_route(&self, route: &TaskRoute) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO task_routes (task_name, entity_id, priority)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![route.task_name, route.entity_id, route.priority],
        )?;
        Ok(())
    }

    pub fn routes_for_task(&self, task_name: &str) -> Result<Vec<TaskRoute>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_name, entity_id, priority
             FROM task_routes WHERE task_name = ?1 ORDER BY priority DESC",
        )?;
        let rows = stmt.query_map([task_name], |row| {
            Ok(TaskRoute {
                task_name: row.get(0)?,
                entity_id: row.get(1)?,
                priority: row.get(2)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn routes_for_entity(&self, entity_id: &str) -> Result<Vec<TaskRoute>> {
        let mut stmt = self.conn.prepare(
            "SELECT task_name, entity_id, priority
             FROM task_routes WHERE entity_id = ?1 ORDER BY priority DESC",
        )?;
        let rows = stmt.query_map([entity_id], |row| {
            Ok(TaskRoute {
                task_name: row.get(0)?,
                entity_id: row.get(1)?,
                priority: row.get(2)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn delete_task_route(&self, task_name: &str, entity_id: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM task_routes WHERE task_name = ?1 AND entity_id = ?2",
            rusqlite::params![task_name, entity_id],
        )?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query_routes() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_task_route(&TaskRoute {
                task_name: "build".to_string(),
                entity_id: "comp::a".to_string(),
                priority: 10,
            })
            .unwrap();
        store
            .insert_task_route(&TaskRoute {
                task_name: "build".to_string(),
                entity_id: "comp::b".to_string(),
                priority: 5,
            })
            .unwrap();
        store
            .insert_task_route(&TaskRoute {
                task_name: "test".to_string(),
                entity_id: "comp::a".to_string(),
                priority: 8,
            })
            .unwrap();

        let build_routes = store.routes_for_task("build").unwrap();
        assert_eq!(build_routes.len(), 2);
        assert_eq!(build_routes[0].entity_id, "comp::a"); // higher priority first

        let entity_routes = store.routes_for_entity("comp::a").unwrap();
        assert_eq!(entity_routes.len(), 2);
    }

    #[test]
    fn delete_route() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_task_route(&TaskRoute {
                task_name: "build".to_string(),
                entity_id: "comp::x".to_string(),
                priority: 1,
            })
            .unwrap();
        assert!(store.delete_task_route("build", "comp::x").unwrap());
        assert!(!store.delete_task_route("build", "comp::x").unwrap());
    }
}
