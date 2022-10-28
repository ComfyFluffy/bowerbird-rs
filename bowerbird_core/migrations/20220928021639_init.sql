create table pixiv_media
(
    id          serial
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
    id       serial
        constraint pixiv_media_color_pk
            primary key,
    media_id integer
        constraint pixiv_media_color_pixiv_media_null_fk
            references pixiv_media,
    h        double precision,
    s        double precision,
    v        double precision
);


create table pixiv_tag
(
    id    serial
        constraint pixiv_tag_pk
            primary key,
    alias varchar(128)[]
);
create index pixiv_tag_alias_index
    on pixiv_tag using gin (alias);


create table pixiv_user
(
    id                     serial
        constraint pixiv_user_pk
            primary key,
    source_id              varchar(24),
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
    id                 serial
        constraint pixiv_user_history_pk
            primary key,
    item_id            integer
        constraint pixiv_user_history_pixiv_user_null_fk
            references pixiv_user,
    workspace_image_id integer
        constraint pixiv_user_history_pixiv_media_null_fk_workspace_image_id
            references pixiv_media,
    background_id      integer
        constraint pixiv_user_history_pixiv_media_null_fk_background_id
            references pixiv_media,
    avatar_id          integer
        constraint pixiv_user_history_pixiv_media_null_fk_avatar_id
            references pixiv_media,
    inserted_at        timestamp with time zone default now(),
    account            varchar(128),
    name               varchar(128),
    is_premium         boolean,
    birth              date,
    region             varchar(64),
    gender             varchar(8),
    comment            text,
    twitter_account    varchar(64),
    web_page           varchar(256),
    workspace          jsonb
);


create table pixiv_illust
(
    id                  serial
        constraint pixiv_illust_pk
            primary key,
    parent_id           integer
        constraint pixiv_illust_pixiv_user_null_fk
            references pixiv_user,
    source_id           varchar(24),
    inserted_at         timestamp with time zone default now(),
    updated_at          timestamp with time zone,
    source_inaccessible boolean                  default false not null,
    total_bookmarks     integer,
    total_view          integer,
    is_bookmarked       boolean,
    tag_ids             integer[]
);
create unique index pixiv_illust_source_id_uindex
    on pixiv_illust (source_id);

create table pixiv_illust_history
(
    id                    serial
        constraint pixiv_illust_history_pk
            primary key,
    item_id               integer
        constraint pixiv_illust_history_pixiv_illust_null_fk
            references pixiv_illust,
    inserted_at           timestamp with time zone default now(),
    illust_type           varchar(10),
    caption_html          text,
    title                 varchar(256),
    date                  timestamp with time zone,
    media_ids             integer[],
    ugoira_frame_duration integer[]
);


create table pixiv_novel
(
    id                  serial
        constraint pixiv_novel_pk
            primary key,
    parent_id           integer
        constraint pixiv_novel_pixiv_user_null_fk
            references pixiv_user,
    source_id           varchar(24),
    inserted_at         timestamp with time zone default now(),
    updated_at          timestamp with time zone,
    source_inaccessible boolean                  default false not null,
    total_bookmarks     integer,
    total_view          integer,
    is_bookmarked       boolean,
    tag_ids             integer[]
);
create unique index pixiv_novel_source_id_uindex
    on pixiv_novel (source_id);

create table pixiv_novel_history
(
    id             serial
        constraint pixiv_novel_history_pk
            primary key,
    item_id        integer
        constraint pixiv_novel_history_pixiv_novel_null_fk
            references pixiv_novel,
    cover_image_id integer
        constraint pixiv_novel_history_pixiv_media_null_fk
            references pixiv_media,
    inserted_at    timestamp with time zone default now(),
    title          varchar(256),
    caption_html   text,
    text           text,
    date           timestamp
);
