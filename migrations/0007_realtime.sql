-- Minimal content-free invalidations. Multi-resource runtime mutations acquire the
-- owning user before resource rows; the clock then serializes publication. The
-- leaf photo API may use photo->clock (no path takes clock then writes photos).
-- Do not replace this clock with a global identity sequence: allocation order is
-- not commit order and a reconnect could otherwise miss a late commit.
CREATE TABLE realtime_clocks (
    owner_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    last_seq bigint NOT NULL CHECK (last_seq > 0)
);
CREATE TABLE realtime_events (
    owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    seq bigint NOT NULL CHECK (seq > 0),
    resource text NOT NULL CHECK (resource IN ('chat','project','memory','memory_settings','account')),
    resource_id uuid NOT NULL,
    action text NOT NULL CHECK (action IN ('invalidate','remove')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(owner_id,seq)
);

CREATE FUNCTION record_realtime_change(p_owner uuid,p_resource text,p_id uuid,p_action text)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE n bigint;
BEGIN
    INSERT INTO realtime_clocks(owner_id,last_seq) VALUES(p_owner,1)
    ON CONFLICT(owner_id) DO UPDATE SET last_seq=realtime_clocks.last_seq+1
    RETURNING last_seq INTO n;
    INSERT INTO realtime_events(owner_id,seq,resource,resource_id,action)
    VALUES(p_owner,n,p_resource,p_id,p_action);
    -- At most 1000 existing rows per owner: deletion work is bounded, including
    -- after a long idle period. Expiry is also enforced by the read query.
    DELETE FROM realtime_events WHERE owner_id=p_owner
      AND (seq<=n-1000 OR created_at<now()-interval '7 days');
END $$;

CREATE FUNCTION realtime_resource_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner uuid; entity uuid; kind text; operation text := 'invalidate';
BEGIN
    IF TG_TABLE_NAME='users' THEN
        owner:=NEW.id; entity:=NEW.id;
        IF TG_OP='INSERT' OR ROW(NEW.email,NEW.display_name,NEW.name_source) IS DISTINCT FROM ROW(OLD.email,OLD.display_name,OLD.name_source) THEN
            PERFORM record_realtime_change(owner,'account',entity,'invalidate');
        END IF;
        IF TG_OP='UPDATE' AND ROW(NEW.memory_enabled,NEW.memory_version) IS DISTINCT FROM ROW(OLD.memory_enabled,OLD.memory_version) THEN
            PERFORM record_realtime_change(owner,'memory_settings',entity,'invalidate');
        END IF;
        RETURN NEW;
    ELSIF TG_TABLE_NAME='account_identities' THEN
        IF TG_OP='UPDATE' AND ROW(NEW.verified_email,NEW.provider_name,NEW.picture_url) IS NOT DISTINCT FROM ROW(OLD.verified_email,OLD.provider_name,OLD.picture_url) THEN
            RETURN NEW;
        END IF;
        IF TG_OP='DELETE' THEN owner:=OLD.user_id; ELSE owner:=NEW.user_id; END IF;
        entity:=owner; kind:='account';
    ELSIF TG_TABLE_NAME='user_profile_photos' THEN
        IF TG_OP='UPDATE' AND NEW.bytes=OLD.bytes THEN RETURN NEW; END IF;
        IF TG_OP='DELETE' THEN owner:=OLD.user_id; ELSE owner:=NEW.user_id; END IF;
        entity:=owner; kind:='account';
    ELSE
        IF TG_OP='DELETE' THEN
            owner:=OLD.owner_id; entity:=OLD.id; operation:='remove';
        ELSE
            owner:=NEW.owner_id; entity:=NEW.id;
            IF NEW.deleted_at IS NOT NULL THEN operation:='remove'; END IF;
        END IF;
        IF TG_TABLE_NAME='chats' THEN
            kind:='chat';
            IF TG_OP='UPDATE' AND ROW(NEW.title,NEW.archived,NEW.deleted_at,NEW.head_id,NEW.project_id) IS NOT DISTINCT FROM ROW(OLD.title,OLD.archived,OLD.deleted_at,OLD.head_id,OLD.project_id) THEN RETURN NEW; END IF;
        ELSIF TG_TABLE_NAME='projects' THEN
            kind:='project';
            IF TG_OP='UPDATE' AND ROW(NEW.name,NEW.deleted_at) IS NOT DISTINCT FROM ROW(OLD.name,OLD.deleted_at) THEN RETURN NEW; END IF;
        ELSE
            kind:='memory';
            IF TG_OP='UPDATE' AND ROW(NEW.content,NEW.deleted_at) IS NOT DISTINCT FROM ROW(OLD.content,OLD.deleted_at) THEN RETURN NEW; END IF;
        END IF;
    END IF;
    -- Account cascade deletion has no session left to notify.
    IF EXISTS(SELECT 1 FROM users WHERE id=owner) THEN
        PERFORM record_realtime_change(owner,kind,entity,operation);
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END $$;

CREATE TRIGGER users_realtime AFTER INSERT OR UPDATE OF email,display_name,name_source,memory_enabled,memory_version ON users FOR EACH ROW EXECUTE FUNCTION realtime_resource_trigger();
CREATE TRIGGER chats_realtime AFTER INSERT OR UPDATE OF title,archived,deleted_at,head_id,project_id OR DELETE ON chats FOR EACH ROW EXECUTE FUNCTION realtime_resource_trigger();
CREATE TRIGGER projects_realtime AFTER INSERT OR UPDATE OF name,deleted_at OR DELETE ON projects FOR EACH ROW EXECUTE FUNCTION realtime_resource_trigger();
CREATE TRIGGER memories_realtime AFTER INSERT OR UPDATE OF content,deleted_at OR DELETE ON memories FOR EACH ROW EXECUTE FUNCTION realtime_resource_trigger();
CREATE TRIGGER identities_realtime AFTER INSERT OR UPDATE OR DELETE ON account_identities FOR EACH ROW EXECUTE FUNCTION realtime_resource_trigger();
CREATE TRIGGER photos_realtime AFTER INSERT OR UPDATE OR DELETE ON user_profile_photos FOR EACH ROW EXECUTE FUNCTION realtime_resource_trigger();
