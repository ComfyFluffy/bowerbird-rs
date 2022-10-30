create view public.pixiv_illust_latest as
select i.id                         id,
       i.parent_id                  parent_id,
       h.id                         history_id,
       i.inserted_at                inserted_at,
       i.updated_at                 updated_at,
       source_id,
       source_inaccessible,
       tag_ids,
       total_bookmarks,
       total_view,
       is_bookmarked,
       illust_type,
       caption_html,
       title,
       date,
       ugoira_frame_duration,
       (select array_agg(local_path order by hm.id)
        from pixiv_media m
                 join pixiv_illust_history_media hm on m.id = hm.media_id
        where hm.history_id = h.id) image_paths

from pixiv_illust_history h
         join (select max(id) id, item_id from pixiv_illust_history group by item_id) sub using (id, item_id)
         join pixiv_illust i on i.id = h.item_id;

create view public.pixiv_user_latest as
select i.id                                                                 id,
       h.id                                                                 history_id,
       i.inserted_at                                                        inserted_at,
       i.updated_at                                                         updated_at,
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
       (select local_path from pixiv_media where id = h.workspace_image_id) workspace_image_path,
       (select local_path from pixiv_media where id = h.background_id)      background_path,
       (select local_path from pixiv_media where id = h.avatar_id)          avatar_path,
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
         join pixiv_user i on i.id = h.item_id;
