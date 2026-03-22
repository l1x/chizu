use std::collections::HashMap;

use grafeo::Value;

use super::entities::val_to_string;
use super::GrafeoStore;
use crate::error::{GraknoError, Result};
use crate::model::TaskRoute;

impl GrafeoStore {
    pub fn insert_task_route(&self, route: &TaskRoute) -> Result<()> {
        let sess = self.session();

        // Delete existing (upsert on composite key)
        let mut params = HashMap::new();
        params.insert(
            "task_name".to_string(),
            Value::from(route.task_name.as_str()),
        );
        params.insert(
            "entity_id".to_string(),
            Value::from(route.entity_id.as_str()),
        );
        sess.execute_with_params(
            "MATCH (n:task_route) WHERE n.task_name = $task_name AND n.entity_id = $entity_id DETACH DELETE n",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let props: Vec<(&str, Value)> = vec![
            ("task_name", Value::from(route.task_name.as_str())),
            ("entity_id", Value::from(route.entity_id.as_str())),
            ("priority", Value::from(route.priority)),
        ];

        sess.create_node_with_props(&["task_route"], props);
        Ok(())
    }

    pub fn routes_for_task(&self, task_name: &str) -> Result<Vec<TaskRoute>> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("task_name".to_string(), Value::from(task_name));
        let result = sess
            .execute_with_params(
                "MATCH (n:task_route) WHERE n.task_name = $task_name RETURN n.task_name, n.entity_id, n.priority ORDER BY n.priority DESC",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_task_route(r)).collect()
    }

    pub fn routes_for_entity(&self, entity_id: &str) -> Result<Vec<TaskRoute>> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("entity_id".to_string(), Value::from(entity_id));
        let result = sess
            .execute_with_params(
                "MATCH (n:task_route) WHERE n.entity_id = $entity_id RETURN n.task_name, n.entity_id, n.priority ORDER BY n.priority DESC",
                params,
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;

        let rows: Vec<Vec<Value>> = result.rows;
        rows.iter().map(|r| row_to_task_route(r)).collect()
    }

    pub fn delete_task_route(&self, task_name: &str, entity_id: &str) -> Result<bool> {
        let sess = self.session();
        let mut params = HashMap::new();
        params.insert("task_name".to_string(), Value::from(task_name));
        params.insert("entity_id".to_string(), Value::from(entity_id));

        let result = sess
            .execute_with_params(
                "MATCH (n:task_route) WHERE n.task_name = $task_name AND n.entity_id = $entity_id RETURN n.task_name",
                params.clone(),
            )
            .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        let rows: Vec<Vec<Value>> = result.rows;
        if rows.is_empty() {
            return Ok(false);
        }

        sess.execute_with_params(
            "MATCH (n:task_route) WHERE n.task_name = $task_name AND n.entity_id = $entity_id DETACH DELETE n",
            params,
        )
        .map_err(|e| GraknoError::Other(format!("grafeo: {e}")))?;
        Ok(true)
    }
}

fn row_to_task_route(row: &[Value]) -> Result<TaskRoute> {
    Ok(TaskRoute {
        task_name: val_to_string(&row[0]),
        entity_id: val_to_string(&row[1]),
        priority: row[2].as_int64().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::grafeo::GrafeoStore;

    #[test]
    fn insert_and_query_task_routes() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let route = TaskRoute {
            task_name: "build".to_string(),
            entity_id: "comp::core".to_string(),
            priority: 10,
        };
        store.insert_task_route(&route).unwrap();

        let by_task = store.routes_for_task("build").unwrap();
        assert_eq!(by_task.len(), 1);
        assert_eq!(by_task[0].entity_id, "comp::core");

        let by_entity = store.routes_for_entity("comp::core").unwrap();
        assert_eq!(by_entity.len(), 1);
        assert_eq!(by_entity[0].task_name, "build");
    }

    #[test]
    fn delete_task_route() {
        let store = GrafeoStore::open_in_memory().unwrap();
        let route = TaskRoute {
            task_name: "test".to_string(),
            entity_id: "comp::x".to_string(),
            priority: 5,
        };
        store.insert_task_route(&route).unwrap();
        assert!(store.delete_task_route("test", "comp::x").unwrap());
        assert!(!store.delete_task_route("test", "comp::x").unwrap());
    }
}
