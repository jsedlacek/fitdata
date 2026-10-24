# Creates the BigQuery tables from deploy/schema/ and adds new columns to
# existing ones (sourced by setup.sh and deploy.sh after config.sh).
apply_schemas() {
  local schema t table
  for schema in deploy/schema/*.json; do
    t=$(basename "$schema" .json)
    for table in "$t" "_staging_$t"; do
      if bq --project_id "$PROJECT" show "$DATASET.$table" >/dev/null 2>&1; then
        # Adds new columns; BigQuery refuses incompatible changes.
        bq --project_id "$PROJECT" update "$PROJECT:$DATASET.$table" "$schema" >/dev/null
      elif [[ $table == _staging_* ]]; then
        bq --project_id "$PROJECT" mk --table "$PROJECT:$DATASET.$table" "$schema" >/dev/null
      else
        bq --project_id "$PROJECT" mk --table --time_partitioning_field date --time_partitioning_type DAY \
          "$PROJECT:$DATASET.$table" "$schema" >/dev/null
      fi
      echo "  $table"
    done
  done
}
