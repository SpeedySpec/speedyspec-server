-- SpeedySpec's support objects for MySQL statements on PostgreSQL, installed once per schema.
CREATE COLLATION IF NOT EXISTS speedyspec_ci (provider = icu, locale = 'und-u-ks-level1', deterministic = false);

-- CAST(x AS SIGNED): MySQL reads the leading integer, else 0.
CREATE OR REPLACE FUNCTION speedyspec_signed(x text) RETURNS bigint LANGUAGE plpgsql IMMUTABLE RETURNS NULL ON NULL INPUT AS $$
DECLARE p text := substring(btrim(x) COLLATE "C" from '^[+-]?[0-9]+');
BEGIN
    IF p IS NULL THEN RETURN 0; END IF;
    RETURN p::bigint;
EXCEPTION WHEN numeric_value_out_of_range THEN
    RETURN CASE WHEN p LIKE '-%' THEN -9223372036854775808 ELSE 9223372036854775807 END;
END $$;

-- CAST(x AS DECIMAL): the leading number, else 0.
CREATE OR REPLACE FUNCTION speedyspec_decimal(x text) RETURNS numeric LANGUAGE plpgsql IMMUTABLE RETURNS NULL ON NULL INPUT AS $$
DECLARE p text := substring(btrim(x) COLLATE "C" from '^[+-]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?');
BEGIN
    IF p IS NULL THEN RETURN 0; END IF;
    RETURN p::numeric;
END $$;

-- A string compared with a number: MySQL compares the string's leading number as a double.
CREATE OR REPLACE FUNCTION speedyspec_double(x text) RETURNS double precision LANGUAGE plpgsql IMMUTABLE RETURNS NULL ON NULL INPUT AS $$
DECLARE p text := substring(btrim(x) COLLATE "C" from '^[+-]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?');
BEGIN
    IF p IS NULL THEN RETURN 0; END IF;
    RETURN p::double precision;
END $$;

-- CAST(x AS DATE) and AS DATETIME: an invalid value is NULL, as in MySQL.
CREATE OR REPLACE FUNCTION speedyspec_date(x text) RETURNS date LANGUAGE plpgsql IMMUTABLE RETURNS NULL ON NULL INPUT AS $$
BEGIN
    RETURN x::date;
EXCEPTION WHEN others THEN
    RETURN NULL;
END $$;

CREATE OR REPLACE FUNCTION speedyspec_datetime(x text) RETURNS timestamp(0) LANGUAGE plpgsql IMMUTABLE RETURNS NULL ON NULL INPUT AS $$
BEGIN
    RETURN x::timestamp(0);
EXCEPTION WHEN others THEN
    RETURN NULL;
END $$;

-- @@name: the session variable the store keeps as speedyspec.<name>.
CREATE OR REPLACE FUNCTION speedyspec_sysvar(name text) RETURNS text LANGUAGE sql STABLE AS $$
    SELECT current_setting('speedyspec.' || name, true)
$$;

-- A value for a latin1 column: MySQL stores '?' for each character latin1 (Windows-1252) cannot hold.
CREATE OR REPLACE FUNCTION speedyspec_latin1(x text) RETURNS text LANGUAGE sql IMMUTABLE RETURNS NULL ON NULL INPUT AS $$
    SELECT regexp_replace(x COLLATE "C", '[^\u0001-\u007f\u00a0-\u00ff€‚ƒ„…†‡ˆ‰Š‹ŒŽ‘’“”•–—˜™š›œžŸ]', '?', 'g')
$$;

COMMENT ON FUNCTION speedyspec_signed(text) IS 'speedyspec-support 4';
