CREATE TABLE IF NOT EXISTS records (
            id INTEGER PRIMARY KEY,
            created_at_ms INTEGER NOT NULL,
            elapsed_ms INTEGER NOT NULL,
            request_id INTEGER NOT NULL,
            client_ip TEXT NOT NULL,
            questions_json TEXT NOT NULL,
            req_rd INTEGER NOT NULL,
            req_cd INTEGER NOT NULL,
            req_ad INTEGER NOT NULL,
            req_opcode TEXT NOT NULL,
            req_edns_json TEXT NULL,
            error TEXT NULL,
            has_response INTEGER NOT NULL,
            rcode TEXT NULL,
            resp_aa INTEGER NULL,
            resp_tc INTEGER NULL,
            resp_ra INTEGER NULL,
            resp_ad INTEGER NULL,
            resp_cd INTEGER NULL,
            answer_count INTEGER NOT NULL,
            authority_count INTEGER NOT NULL,
            additional_count INTEGER NOT NULL,
            answers_json TEXT NOT NULL,
            authorities_json TEXT NOT NULL,
            additionals_json TEXT NOT NULL,
            signature_json TEXT NOT NULL,
            resp_edns_json TEXT NULL
        );
        CREATE TABLE IF NOT EXISTS steps (
            record_id INTEGER NOT NULL,
            event_index INTEGER NOT NULL,
            sequence_tag TEXT NOT NULL,
            node_index INTEGER NULL,
            kind TEXT NOT NULL,
            tag TEXT NULL,
            outcome TEXT NOT NULL,
            PRIMARY KEY (record_id, event_index),
            FOREIGN KEY(record_id) REFERENCES records(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS questions (
            record_id INTEGER NOT NULL,
            question_index INTEGER NOT NULL,
            name_lc TEXT NOT NULL,
            qtype TEXT NOT NULL,
            qclass TEXT NOT NULL,
            PRIMARY KEY (record_id, question_index),
            FOREIGN KEY(record_id) REFERENCES records(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS records_created_at_idx ON records(created_at_ms DESC);
        CREATE INDEX IF NOT EXISTS records_request_id_idx ON records(request_id);
        CREATE INDEX IF NOT EXISTS records_client_ip_idx ON records(client_ip);
        CREATE INDEX IF NOT EXISTS records_rcode_idx ON records(rcode);
        CREATE INDEX IF NOT EXISTS questions_record_id_idx ON questions(record_id);
        CREATE INDEX IF NOT EXISTS questions_name_idx ON questions(name_lc, record_id);
        CREATE INDEX IF NOT EXISTS questions_qtype_idx ON questions(qtype, record_id);
        CREATE INDEX IF NOT EXISTS steps_kind_tag_outcome_idx ON steps(kind, tag, outcome);
        CREATE INDEX IF NOT EXISTS steps_record_id_idx ON steps(record_id);
        -- Covering index for the matcher_tag EXISTS subquery used by /records
        -- and /stats endpoints. Without record_id in the index tail SQLite
        -- has to do a second lookup per candidate, which makes rapid
        -- matcher-click filtering pile up in the blocking pool.
        CREATE INDEX IF NOT EXISTS steps_matcher_lookup_idx
            ON steps(kind, tag, outcome, record_id);
        -- Speeds up `/stats/plugins` style JOINs which find `s.record_id = r.id
        -- AND s.kind = ?`. With only the single-column record_id index the
        -- planner reads every step for that record and filters by kind in
        -- memory; the (record_id, kind) prefix turns that into a covered
        -- range lookup.
        CREATE INDEX IF NOT EXISTS steps_record_kind_idx
            ON steps(record_id, kind);
