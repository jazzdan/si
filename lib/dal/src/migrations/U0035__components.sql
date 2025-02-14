CREATE TABLE components
(
    pk                          ident                    PRIMARY KEY DEFAULT ident_create_v1(),
    id                          ident                    NOT NULL DEFAULT ident_create_v1(),
    tenancy_workspace_pk        ident,
    visibility_change_set_pk    ident                    NOT NULL DEFAULT ident_nil_v1(),
    visibility_deleted_at       timestamp with time zone,
    created_at                  timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    creation_user_pk            ident,
    updated_at                  timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    kind                        text                     NOT NULL,
    deletion_user_pk            ident,
    needs_destroy               bool                     NOT NULL DEFAULT false
);
SELECT standard_model_table_constraints_v1('components');
SELECT belongs_to_table_create_v1('component_belongs_to_schema', 'components', 'schemas');
SELECT belongs_to_table_create_v1('component_belongs_to_schema_variant', 'components', 'schema_variants');

INSERT INTO standard_models (table_name, table_type, history_event_label_base, history_event_message_name)
VALUES ('components', 'model', 'component', 'Component'),
       ('component_belongs_to_schema', 'belongs_to', 'component.schema', 'Component <> Schema'),
       ('component_belongs_to_schema_variant', 'belongs_to', 'component.schema_variant', 'Component <> SchemaVariant');


CREATE TABLE component_statuses
(
    pk                          ident                    PRIMARY KEY DEFAULT ident_create_v1(),
    id                          ident                    NOT NULL,
    tenancy_workspace_pk        ident,
    visibility_change_set_pk    ident                    NOT NULL DEFAULT ident_nil_v1(),
    visibility_deleted_at       timestamp with time zone,
    created_at                  timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    updated_at                  timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    creation_timestamp          timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    creation_user_pk            ident,
    update_timestamp            timestamp with time zone NOT NULL DEFAULT CLOCK_TIMESTAMP(),
    update_user_pk              ident
);
SELECT standard_model_table_constraints_v1('component_statuses');
INSERT INTO standard_models (table_name, table_type, history_event_label_base, history_event_message_name)
VALUES ('component_statuses', 'model', 'component_status', 'Component Status');

CREATE OR REPLACE FUNCTION component_create_v1(
    this_tenancy jsonb,
    this_visibility jsonb,
    this_user_pk ident,
    this_kind text,
    OUT object json) AS
$$
DECLARE
    this_tenancy_record    tenancy_record_v1;
    this_visibility_record visibility_record_v1;
    this_new_row           components%ROWTYPE;
BEGIN
    this_tenancy_record := tenancy_json_to_columns_v1(this_tenancy);
    this_visibility_record := visibility_json_to_columns_v1(this_visibility);

    INSERT INTO components (tenancy_workspace_pk,
                            visibility_change_set_pk, kind, creation_user_pk)
    VALUES (this_tenancy_record.tenancy_workspace_pk,
            this_visibility_record.visibility_change_set_pk, this_kind,
            this_user_pk)
    RETURNING * INTO this_new_row;

    -- Create a parallel record to store creation and update status, meaning that this table's id refers to components.id
    INSERT INTO component_statuses (id,
                                    tenancy_workspace_pk,
                                    visibility_change_set_pk,
                                    creation_user_pk, update_user_pk)
    VALUES (this_new_row.id,
            this_new_row.tenancy_workspace_pk,
            this_new_row.visibility_change_set_pk,
            this_user_pk, this_user_pk);

    object := row_to_json(this_new_row);
END;
$$ LANGUAGE PLPGSQL VOLATILE;
