create or replace view pixiv_novel_detail_latest_view as
select i.id          as id,
       i.parent_id   as parent_id,
       h.id          as history_id,
       i.inserted_at as inserted_at,
       i.updated_at  as updated_at,
       i.source_id,
       source_inaccessible,
       tag_ids,
       total_bookmarks,
       total_view,
       is_bookmarked,
       h.title,
       caption_html,
       text,
       date,
       s.id          as series_id,
       s.title       as series_title
from pixiv_novel_history h
         join (select max(id) id from pixiv_novel_history group by item_id) max_id on max_id.id = h.id
         join pixiv_novel i on i.id = h.item_id
         left join pixiv.novel_series s on s.id = i.series_id
;
