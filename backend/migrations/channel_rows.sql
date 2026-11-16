-- 私信的两端在同一实例上时，同一个 channel_id 有两行，每人一行：状态和 E2E 密钥各管各的。
-- 消息还是一行，两人共用，所以消息表不能再用外键指向频道表：
-- 最后一行频道没了，消息才跟着走。用户注销的级联和手动删除都经过这里。

DO $$
DECLARE
    old record;
BEGIN
    FOR old IN
        SELECT con.conname
        FROM pg_constraint con
        JOIN pg_attribute att
          ON att.attrelid = con.conrelid AND att.attnum = con.conkey[1]
        WHERE con.conrelid = 'federation_channels'::regclass
          AND con.contype = 'u'
          AND array_length(con.conkey, 1) = 1
          AND att.attname = 'channel_id'
    LOOP
        -- CASCADE 同时带走依赖它的外键 fk_fed_channel_messages_channel。
        EXECUTE format('ALTER TABLE federation_channels DROP CONSTRAINT %I CASCADE', old.conname);
    END LOOP;
END $$;
DROP INDEX IF EXISTS federation_channels_channel_id_key CASCADE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_channels_channel_user
    ON federation_channels (channel_id, user_id);

CREATE OR REPLACE FUNCTION federation_channel_row_gone() RETURNS trigger AS $$
BEGIN
    -- 两人同时删时，各自都看不到对方未提交的删除，这里排个队。
    PERFORM pg_advisory_xact_lock(hashtext(OLD.channel_id));
    IF NOT EXISTS (SELECT 1 FROM federation_channels WHERE channel_id = OLD.channel_id) THEN
        DELETE FROM federation_channel_messages WHERE channel_id = OLD.channel_id;
    END IF;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS federation_channel_row_gone ON federation_channels;
CREATE TRIGGER federation_channel_row_gone
    AFTER DELETE ON federation_channels
    FOR EACH ROW EXECUTE FUNCTION federation_channel_row_gone();
