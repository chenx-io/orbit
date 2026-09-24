//! DB/Redis assertions: YAML parsing smoke test (matching the shape of examples/db-assertion.yaml).
//! The execution/evaluation logic is covered by orbit-assertion unit tests (MockProvider).

use orbit_config::Step;

#[test]
fn parses_db_and_redis_checks_with_meta() {
    let yaml = r#"
name: "db-assertion-demo"
variables:
  base_url: "http://127.0.0.1:8080"
scenarios:
  - name: "s"
    executor: { type: sequential, iterations: 1 }
    steps:
      - name: "create"
        type: request
        request: { method: POST, url: "{{base_url}}/orders" }
        checks:
          - type: status
            value: 200
          - type: jsonpath
            path: "orderId"
            comparator: exists
          - type: db
            meta: { name: "persisted", enabled: true }
            datasource: "orders-db"
            sql: "SELECT status FROM orders WHERE id = '{{orderId}}'"
            target: { type: cell, row: 0, column: "status" }
            comparator: equal
            expected: "PAID"
            retry: { interval_ms: 500, max_attempts: 10, timeout_ms: 5000 }
            extract_var: "dbStatus"
            hard: true
          - type: redis
            datasource: "cache-redis"
            command: TTL
            args: ["order:{{orderId}}"]
            comparator: gt
            expected: "0"
            retry: { interval_ms: 300, max_attempts: 5 }
        extract:
          - { name: orderId, from: jsonpath, path: "orderId" }
"#;
    let plan = orbit_config::from_str(yaml).expect("YAML should parse successfully");
    let steps = &plan.scenarios[0].steps;
    assert_eq!(steps.len(), 1);
    let Step::Request {
        checks, extract, ..
    } = &steps[0]
    else {
        panic!("expected a Request step");
    };
    assert_eq!(checks.len(), 4, "status + jsonpath + db + redis");
    assert_eq!(extract.len(), 1);
    let db = &checks[2];
    let meta = db.meta.as_ref().expect("a db check should carry meta");
    assert_eq!(meta.name.as_deref(), Some("persisted"));
    assert!(db.meta.as_ref().map(|m| m.enabled).unwrap_or(true));
}
