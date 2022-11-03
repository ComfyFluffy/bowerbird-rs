create view pixiv_user_latest as
select i.id          as id,
       h.id             history_id,
       i.inserted_at as inserted_at,
       i.updated_at  as updated_at,
       source_id,
       source_inaccessible,
       is_followed,
       total_following,
       total_illust_series,
       total_illusts,
       total_manga,
       total_novel_series,
       total_novels,
       total_public_bookmarks,
       mw.url           workspace_image_url,
       mw.local_path    workspace_image_path,
       mb.url           background_url,
       mb.local_path    background_path,
       ma.url           avatar_url,
       ma.local_path    avatar_path,
       account,
       name,
       is_premium,
       birth,
       region,
       gender,
       comment,
       twitter_account,
       web_page,
       workspace

from pixiv_user_history h
         join (select max(id) id, item_id from pixiv_user_history group by item_id) sub using (id, item_id)
         join pixiv_user i on i.id = h.item_id
         left join pixiv_media ma on ma.id = h.avatar_id
         left join pixiv_media mb on mb.id = h.background_id
         left join pixiv_media mw on mw.id = h.workspace_image_id;

create view pixiv_illust_latest as
select i.id          as                                                id,
       i.parent_id   as                                                parent_id,
       h.id          as                                                history_id,
       i.inserted_at as                                                inserted_at,
       i.updated_at  as                                                updated_at,
       source_id,
       source_inaccessible,
       tag_ids,
       total_bookmarks,
       total_view,
       is_bookmarked,
       (select name from pixiv_illust_history_type where id = type_id) illust_type,
       title,
       caption_html,
       date,
       ugoira_frame_duration,
       m.paths                                                         image_paths,
       m.urls                                                          image_urls

from pixiv_illust_history h
         join (select max(id) id, item_id from pixiv_illust_history group by item_id) sub using (id, item_id)
         join pixiv_illust i on i.id = h.item_id
         left join (select hm.history_id                        history_id,
                           array_agg(url order by hm.id)        urls,
                           array_agg(local_path order by hm.id) paths
                    from pixiv_media m
                             join pixiv_illust_history_media hm on m.id = hm.media_id
                    group by hm.history_id) m on m.history_id = h.id
;

create view pixiv_novel_latest as
select i.id          as id,
       i.parent_id   as parent_id,
       h.id          as history_id,
       i.inserted_at as inserted_at,
       i.updated_at  as updated_at,
       source_id,
       source_inaccessible,
       tag_ids,
       total_bookmarks,
       total_view,
       is_bookmarked,
       title,
       caption_html,
       text,
       date
from pixiv_novel_history h
         join (select max(id) id, item_id from pixiv_novel_history group by item_id) sub using (id, item_id)
         join pixiv_novel i on i.id = h.item_id;
