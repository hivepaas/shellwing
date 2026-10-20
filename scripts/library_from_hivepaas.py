#!/usr/bin/env python3
"""Builds the snippet library (crates/app/src/library/items.json) from
HivePaaS's command templates (assets/templates/commands in the hivepaas
repository): one item per template, but the `.pipe` ones (streams for
HivePaaS's pipelines, no use typed in a shell).

    scripts/library_from_hivepaas.py ~/go/src/github.com/hivepaas/hivepaas

A template's arguments in use make the command; those not in use become the
item's options, put in at `<options>`. HivePaaS's values become the
snippet's: `{{db_user}}`, `{{database}}` and the like are asked when it runs
(`{{user}}` is the SSH user's, so not that one), the port is the tool's
usual one. Passwords never go in a command: the tool reads its own variable
(PGPASSWORD, MYSQL_PWD…) or `$DB_PASSWORD`, from an environment the snippet
is given.
"""

import glob
import json
import os
import sys

TOOLS = [
    # (template prefix, tool, its usual port)
    ("pg", "PostgreSQL", "5432"),
    ("neon", "PostgreSQL", "5432"),
    ("mysql", "MySQL", "3306"),
    ("sysbench", "MySQL", "3306"),
    ("mariadb", "MariaDB", "3306"),
    ("redis", "Redis", "6379"),
    ("mongo", "MongoDB", "27017"),
    ("clickhouse", "ClickHouse", "9000"),
    ("elasticsearch", "Elasticsearch", "9200"),
    ("influx", "InfluxDB", "8086"),
    ("sqlite", "SQLite", ""),
    ("sqlserver", "SQL Server", "1433"),
    ("dolt", "Dolt", ""),
    ("docker", "Docker", ""),
    ("alembic", "Migrations", ""),
    ("goose", "Migrations", ""),
    ("migrate", "Migrations", ""),
    ("prisma", "Migrations", ""),
    ("hey", "Load tests", ""),
    ("k6", "Load tests", ""),
    ("iperf3", "Load tests", ""),
]

TASKS = {
    "backup": "Backup",
    "data-ops": "Data",
    "database": "Database",
    "diagnostics": "Diagnostics",
    "maintenance": "Maintenance",
    "testing": "Testing",
}

NAMES = {
    "backup/clickhouse-dump": "Back up to an archive",
    "backup/clickhouse-restore": "Restore from an archive",
    "backup/dolt-dump": "Dump to an SQL file",
    "backup/dolt-restore": "Restore from an SQL file",
    "backup/elasticsearch-dump": "Dump an index to a JSON file",
    "backup/elasticsearch-restore": "Restore an index from a JSON file",
    "backup/influx-dump": "Back up to a folder",
    "backup/influx-restore": "Restore from a folder",
    "backup/mariadb-dump": "Dump to an SQL file",
    "backup/mariadb": "Restore from an SQL file",
    "backup/mongodump": "Dump to an archive",
    "backup/mongorestore": "Restore from an archive",
    "backup/mysql": "Restore from an SQL file",
    "backup/mysqldump": "Dump to an SQL file",
    "backup/neon-dump": "Dump a Neon database to a file",
    "backup/neon-restore": "Restore a Neon database from a file",
    "backup/pg_dump": "Dump to a file",
    "backup/pg_restore": "Restore from a dump",
    "backup/redis-dump": "Save a snapshot to an RDB file",
    "backup/redis-restore": "Restore from an RDB file",
    "backup/sqlite-dump": "Back up to a file",
    "backup/sqlite-restore": "Restore from a file",
    "backup/sqlserver-dump": "Back up to a BAK file",
    "backup/sqlserver-restore": "Restore from a BAK file",
    "data-ops/alembic_upgrade_head": "Alembic: upgrade to the latest",
    "data-ops/clickhouse_export_parquet": "Export a table to Parquet",
    "data-ops/clickhouse_import_parquet": "Import Parquet into a table",
    "data-ops/goose_down": "goose: roll back the last",
    "data-ops/goose_up": "goose: apply all",
    "data-ops/migrate_down": "golang-migrate: roll back the last",
    "data-ops/migrate_up": "golang-migrate: apply all",
    "data-ops/mongo_export_json": "Export a collection to JSON",
    "data-ops/mongo_import_json": "Import JSON into a collection",
    "data-ops/mysql_export_csv": "Export a table to CSV",
    "data-ops/mysql_import_csv": "Import CSV into a table",
    "data-ops/pg_export_csv": "Export a table to CSV",
    "data-ops/pg_import_csv": "Import CSV into a table",
    "data-ops/prisma_migrate_deploy": "Prisma: deploy pending migrations",
    "database/clickhouse_create_database": "Create a database",
    "database/mariadb_create_database": "Create a database",
    "database/mariadb_create_user": "Create a user",
    "database/mongo_create_user": "Create a user",
    "database/mongo_drop_database": "Drop a database",
    "database/mysql_create_database": "Create a database",
    "database/mysql_create_user": "Create a user",
    "database/pg_create_database": "Create a database",
    "database/pg_create_user": "Create a user",
    "database/pg_enable_extensions": "Enable common extensions",
    "database/redis_flushall_async": "Delete every key, all databases",
    "database/redis_flushdb_async": "Delete every key of this database",
    "diagnostics/clickhouse_processes": "Running queries",
    "diagnostics/elasticsearch_cluster_health": "Cluster health",
    "diagnostics/mariadb_ping": "Ping",
    "diagnostics/mariadb_processlist": "Running threads",
    "diagnostics/mariadb_table_size": "Table sizes",
    "diagnostics/mongo_db_stats": "Database stats",
    "diagnostics/mongo_ping": "Ping",
    "diagnostics/mongo_server_status": "Server status",
    "diagnostics/mysql_ping": "Ping",
    "diagnostics/mysql_processlist": "Running threads",
    "diagnostics/mysql_table_size": "Table sizes",
    "diagnostics/pg_ping": "Ping",
    "diagnostics/pg_stat_activity": "Active queries",
    "diagnostics/pg_table_size": "Largest tables",
    "diagnostics/redis_bigkeys": "Biggest keys",
    "diagnostics/redis_info": "Server info",
    "diagnostics/redis_ping": "Ping",
    "maintenance/clickhouse_optimize_table": "Optimize a table",
    "maintenance/docker_logs_cleanup": "Empty the containers' logs",
    "maintenance/docker_prune_all": "Remove everything unused",
    "maintenance/elasticsearch_clearcache": "Clear the caches",
    "maintenance/elasticsearch_forcemerge": "Force-merge segments",
    "maintenance/mariadb_analyze": "Analyze tables",
    "maintenance/mariadb_optimize": "Optimize tables",
    "maintenance/mongo_compact": "Compact a collection",
    "maintenance/mysql_analyze": "Analyze tables",
    "maintenance/mysql_optimize": "Optimize tables",
    "maintenance/pg_reindex_database": "Rebuild indexes",
    "maintenance/pg_vacuum_analyze": "Vacuum and analyze",
    "maintenance/redis_bgsave": "Save to disk in the background",
    "maintenance/redis_memory_purge": "Give unused memory back",
    "testing/clickhouse_benchmark": "Benchmark queries",
    "testing/hey_load_test": "HTTP load test (hey)",
    "testing/iperf3_bandwidth": "Network bandwidth (iperf3)",
    "testing/k6_run": "Load test script (k6)",
    "testing/mongo_bench_insert": "Benchmark inserts",
    "testing/mysqlslap_run": "Benchmark queries (mysqlslap)",
    "testing/pgbench_init": "pgbench: set up",
    "testing/pgbench_run": "pgbench: run",
    "testing/redis_benchmark": "Benchmark (redis-benchmark)",
    "testing/redis_benchmark_ping": "Benchmark PING",
    "testing/sysbench_mysql_oltp": "OLTP benchmark (sysbench)",
}

# What these do changes data or schema, or takes things away: typed, not
# run, until the user says so.
CHANGES_DATA = {
    "backup/clickhouse-restore", "backup/dolt-restore", "backup/elasticsearch-restore",
    "backup/influx-restore", "backup/mariadb", "backup/mongorestore", "backup/mysql",
    "backup/neon-restore", "backup/pg_restore", "backup/redis-restore",
    "backup/sqlite-restore", "backup/sqlserver-restore",
    "data-ops/alembic_upgrade_head", "data-ops/clickhouse_import_parquet",
    "data-ops/goose_down", "data-ops/goose_up", "data-ops/migrate_down",
    "data-ops/migrate_up", "data-ops/mongo_import_json", "data-ops/mysql_import_csv",
    "data-ops/pg_import_csv", "data-ops/prisma_migrate_deploy",
    "database/mongo_drop_database", "database/redis_flushall_async",
    "database/redis_flushdb_async", "maintenance/docker_logs_cleanup",
    "maintenance/docker_prune_all", "testing/pgbench_init",
}

VALUES = {
    "HIVEPAAS_USER": "{{db_user}}",
    "HIVEPAAS_DATABASE": "{{database}}",
    "HIVEPAAS_TABLE": "{{table}}",
    "YOUR_TABLE": "{{table}}",
    "HIVEPAAS_TARGET_DB": "{{new_database}}",
    "HIVEPAAS_TARGET_USER": "{{new_user}}",
    "HIVEPAAS_TARGET_PASSWORD": "{{new_password}}",
    "HIVEPAAS_TARGET_HOST": "{{target_host}}",
    "HIVEPAAS_ORG": "{{org}}",
    "HIVEPAAS_COLLECTION": "{{collection}}",
    "DATABASE_URL": "{{database_url}}",
}

SAFE = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_./:=,%+@{}")


def quoted(v):
    """`v` as one word for the shell: bare when it can be, else quoted so
    that only `$DB_PASSWORD` (and nothing else) is expanded."""
    if v and all(c in SAFE for c in v):
        return v
    if "$DB_PASSWORD" in v or "'" in v:
        esc = v.replace("\\", "\\\\").replace('"', '\\"').replace("`", "\\`")
        esc = esc.replace("$", "\\$").replace("\\$DB_PASSWORD", "$DB_PASSWORD")
        return '"' + esc + '"'
    return "'" + v + "'"


def ours(s, port):
    """HivePaaS's values in `s` as the snippet's."""
    s = s.replace("${HIVEPAAS_PASSWORD}", "$DB_PASSWORD").replace("${HIVEPAAS_PORT}", port)
    for k, v in VALUES.items():
        s = s.replace("${%s}" % k, v)
    return s


def tool_of(name):
    for prefix, tool, port in TOOLS:
        if name.startswith(prefix):
            return tool, port
    raise SystemExit(f"no tool for {name}")


def item(path, root):
    key = os.path.relpath(path, root)[:-5]
    group, name = key.split("/")
    tool, port = tool_of(name)
    d = json.load(open(path))
    password = None
    for e in d.get("envVars", []):
        if "PASSWORD" in e["v"]:
            password = e["k"]
    command = d["command"]
    options = []
    groups = d.get("argGroups", [])
    for gi, g in enumerate(groups):
        sep = g.get("separator", " ")
        words = []
        for a in g["args"]:
            if "HIVEPAAS_PASSWORD" in a["name"] + a.get("value", "") and password is None:
                password = "DB_PASSWORD"
            name = quoted(ours(a["name"], port))
            value = a.get("value")
            if value is not None:
                word = name + ("=" if sep == "=" else " ") + quoted(ours(value, port))
            else:
                word = name
            # Never asking: shellwing's user can type a password.
            if a["name"] in ("--no-password", "-w"):
                continue
            (words if a.get("use") else options).append(word)
        text = " ".join(words)
        # Options go at the end of the group they belong to.
        if any(not a.get("use") for a in g["args"]):
            text += "<options>"
        command = command.replace("${%s}" % g["exportEnv"], text)
    for k, v in VALUES.items():
        command = command.replace("${%s}" % k, v)
    command = " ".join(command.split()).replace(" <options>", "<options>")
    if "${" in command:
        raise SystemExit(f"{key}: left over: {command}")
    return {
        "id": key,
        "tool": tool,
        "task": TASKS[group],
        "name": NAMES[key],
        "about": d["desc"],
        "command": command,
        "options": options,
        "password": password,
        "changes_data": key in CHANGES_DATA,
        "docs": d.get("link"),
    }


def main():
    root = os.path.join(sys.argv[1], "assets", "templates", "commands")
    paths = [p for p in sorted(glob.glob(os.path.join(root, "*", "*.json")))
             if not p.endswith(".pipe.json")]
    items = [item(p, root) for p in paths]
    items.sort(key=lambda i: ([t[1] for t in TOOLS].index(i["tool"]),
                              list(TASKS.values()).index(i["task"]), i["name"]))
    out = os.path.join(os.path.dirname(__file__), "..", "crates", "app", "src", "library",
                       "items.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w") as f:
        json.dump({"from": "hivepaas assets/templates/commands", "items": items}, f,
                  indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"{len(items)} items → {os.path.normpath(out)}")


if __name__ == "__main__":
    main()
