create table pixiv_media
(
    id          bigint generated always as identity
        constraint pixiv_media_pk
            primary key,
    url         varchar(256),
    inserted_at timestamp with time zone default now(),
    size        integer,
    mime        varchar(32),
    local_path  varchar(256),
    width       integer,
    height      integer
);
create unique index pixiv_media_url_uindex
    on pixiv_media (url);
create unique index pixiv_media_local_path_uindex
    on pixiv_media (local_path);

create table pixiv_media_color
(
    id       bigint generated always as identity
        constraint pixiv_media_color_pk
            primary key,
    media_id bigint
        constraint pixiv_media_color_pixiv_media_null_fk
            references pixiv_media,
    h        double precision,
    s        double precision,
    v        double precision
);
create index pixiv_media_color_media_id_index
    on pixiv_media_color (media_id);


create table pixiv_tag
(
    id    bigint generated always as identity
        constraint pixiv_tag_pk
            primary key,
    alias varchar(128)[]
);
create index pixiv_tag_alias_index
    on pixiv_tag using gin (alias);


create table pixiv_user
(
    id                     bigint generated always as identity
        constraint pixiv_user_pk
            primary key,
    source_id              varchar(128),
    inserted_at            timestamp with time zone default now(),
    updated_at             timestamp with time zone,
    source_inaccessible    boolean                  default false not null,
    is_followed            boolean,
    total_following        integer,
    total_illust_series    integer,
    total_illusts          integer,
    total_manga            integer,
    total_novel_series     integer,
    total_novels           integer,
    total_public_bookmarks integer
);
create unique index pixiv_user_source_id_uindex
    on pixiv_user (source_id);

create table pixiv_user_history
(
    id                 bigint generated always as identity
        constraint pixiv_user_history_pk
            primary key,
    item_id            bigint
        constraint pixiv_user_history_pixiv_user_null_fk
            references pixiv_user,
    workspace_image_id bigint
        constraint pixiv_user_history_pixiv_media_null_fk_workspace_image_id
            references pixiv_media,
    background_id      bigint
        constraint pixiv_user_history_pixiv_media_null_fk_background_id
            references pixiv_media,
    avatar_id          bigint
        constraint pixiv_user_history_pixiv_media_null_fk_avatar_id
            references pixiv_media,
    inserted_at        timestamp with time zone default now(),
    account            varchar(128),
    name               varchar(128),
    is_premium         boolean,
    birth              date,
    region             varchar(128),
    gender             varchar(8),
    comment            text,
    twitter_account    varchar(128),
    web_page           varchar(512),
    workspace          jsonb
);
create index pixiv_user_history_item_id_index
    on pixiv_user_history (item_id);


create table pixiv_illust
(
    id                  bigint generated always as identity
        constraint pixiv_illust_pk
            primary key,
    parent_id           bigint
        constraint pixiv_illust_pixiv_user_null_fk
            references pixiv_user,
    source_id           varchar(128),
    inserted_at         timestamp with time zone default now(),
    updated_at          timestamp with time zone,
    source_inaccessible boolean                  default false not null,
    total_bookmarks     integer,
    total_view          integer,
    is_bookmarked       boolean,
    tag_ids             bigint[]
);
create index pixiv_illust_parent_id_index
    on pixiv_illust (parent_id);
create unique index pixiv_illust_source_id_uindex
    on pixiv_illust (source_id);

create table pixiv_illust_history_type
(
    id   smallint generated always as identity
        constraint pixiv_illust_history_type_pk
            primary key,
    name varchar(16) not null
);
create unique index pixiv_illust_history_type_name_uindex
    on pixiv_illust_history_type (name);
insert into pixiv_illust_history_type (name)
values ('illust'),
       ('manga'),
       ('ugoira');

create table pixiv_illust_history
(
    id                    bigint generated always as identity
        constraint pixiv_illust_history_pk
            primary key,
    item_id               bigint
        constraint pixiv_illust_history_pixiv_illust_null_fk
            references pixiv_illust,
    type_id               smallint
        constraint pixiv_illust_history_pixiv_illust_history_type_null_fk
            references pixiv_illust_history_type (id),
    inserted_at           timestamp with time zone default now(),
    caption_html          text,
    title                 varchar(256),
    date                  timestamp with time zone,
    ugoira_frame_duration integer[]
);
create index pixiv_illust_history_item_id_index
    on pixiv_illust_history (item_id);
create index pixiv_illust_history_type_id_index
    on pixiv_illust_history (type_id);

create table pixiv_illust_history_media
(
    id         bigint generated always as identity
        constraint pixiv_illust_history_media_pk
            primary key,
    history_id bigint not null
        constraint pixiv_illust_history_media_pixiv_illust_history_null_fk
            references pixiv_illust_history (id),
    media_id   bigint not null
        constraint pixiv_illust_history_media_pixiv_media_null_fk
            references pixiv_media (id)
);
create index pixiv_illust_history_media_history_id_index
    on pixiv_illust_history_media (history_id);
create unique index pixiv_illust_history_media_history_id_media_id_uindex
    on pixiv_illust_history_media (history_id, media_id);
create index pixiv_illust_history_media_media_id_index
    on pixiv_illust_history_media (media_id);


create table pixiv_novel
(
    id                  bigint generated always as identity
        constraint pixiv_novel_pk
            primary key,
    parent_id           bigint
        constraint pixiv_novel_pixiv_user_null_fk
            references pixiv_user,
    source_id           varchar(128),
    inserted_at         timestamp with time zone default now(),
    updated_at          timestamp with time zone,
    source_inaccessible boolean                  default false not null,
    total_bookmarks     integer,
    total_view          integer,
    is_bookmarked       boolean,
    tag_ids             bigint[]
);
create unique index pixiv_novel_source_id_uindex
    on pixiv_novel (source_id);
create index pixiv_novel_parent_id_index
    on pixiv_novel (parent_id);

create table pixiv_novel_history
(
    id             bigint generated always as identity
        constraint pixiv_novel_history_pk
            primary key,
    item_id        bigint
        constraint pixiv_novel_history_pixiv_novel_null_fk
            references pixiv_novel,
    cover_image_id bigint
        constraint pixiv_novel_history_pixiv_media_null_fk
            references pixiv_media,
    inserted_at    timestamp with time zone default now(),
    title          varchar(256),
    caption_html   text,
    text           text,
    date           timestamp with time zone
);
create index pixiv_novel_history_item_id_index
    on pixiv_novel_history (item_id);
