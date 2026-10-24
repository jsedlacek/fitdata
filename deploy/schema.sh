# Creates the BigQuery tables from deploy/schema/ and adds new columns to
# existing ones (sourced by setup.sh and deploy.sh after config.sh).
apply_schemas() {
  local schema table
  for schema in deploy/schema/*.json; do
    table=$(basename "$schema" .json)
    if bq --project_id "$PROJECT" show "$DATASET.$table" >/dev/null 2>&1; then
      # Adds new columns; BigQuery refuses incompatible changes.
      bq --project_id "$PROJECT" update "$PROJECT:$DATASET.$table" "$schema" >/dev/null
    else
      bq --project_id "$PROJECT" mk --table --time_partitioning_field date --time_partitioning_type DAY \
        "$PROJECT:$DATASET.$table" "$schema" >/dev/null
    fi
    echo "  $table"
    # Shared staging tables from before each load made its own (bq.rs).
    if bq --project_id "$PROJECT" show "$DATASET._staging_$table" >/dev/null 2>&1; then
      bq --project_id "$PROJECT" rm -f --table "$PROJECT:$DATASET._staging_$table" >/dev/null
    fi
  done
}
