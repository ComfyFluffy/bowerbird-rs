create or replace function upsert_pixiv_tag(_tag varchar[]) returns void as
$$
declare
    _id bigint;
begin
    select id into _id from pixiv_tag where alias && _tag order by id limit 1;
    if _id is null then
        insert into pixiv_tag (alias) values (_tag);
    else
        update pixiv_tag set alias = array(select distinct unnest(alias || _tag)) where id = _id;
    end if;
end;
$$
    language plpgsql;
