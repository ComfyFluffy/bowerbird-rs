create table pixiv_tag
(
    id    serial
        constraint pixiv_tag_pk
            primary key,
    alias varchar(128)[]
);

alter table pixiv_tag
    owner to postgres;

create index pixiv_tag_alias_index
    on pixiv_tag using gin (alias);

create table pixiv_media
(
    id         serial
        constraint pixiv_media_pk
            primary key,
    url        varchar(256),
    size       integer,
    mime       varchar(32),
    local_path varchar(256),
    width      integer,
    height     integer
);

alter table pixiv_media
    owner to postgres;

create unique index pixiv_media_url_uindex
    on pixiv_media (url);

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

alter table pixiv_media_color
    owner to postgres;

create table pixiv_user
(
    id                     serial
        constraint pixiv_user_pk
            primary key,
    source_id              varchar(24),
    source_inaccessible    boolean,
    last_modified          timestamp,
    is_followed            boolean,
    total_following        integer,
    total_illust_series    integer,
    total_illusts          integer,
    total_manga            integer,
    total_novel_series     integer,
    total_novels           integer,
    total_public_bookmarks integer
);

alter table pixiv_user
    owner to postgres;

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
    last_modified      timestamp,
    birth              date,
    region             varchar(64),
    gender             varchar(8),
    comment            text,
    twitter_account    varchar(64),
    web_page           varchar(256),
    workspace          jsonb
);

alter table pixiv_user_history
    owner to postgres;

create table pixiv_illust
(
    id                  serial
        constraint pixiv_illust_pk
            primary key,
    parent_id           integer
        constraint pixiv_illust_pixiv_user_null_fk
            references pixiv_user,
    source_id           varchar(24),
    source_inaccessible boolean,
    last_modified       timestamp,
    total_bookmarks     integer,
    total_view          integer,
    is_bookmarked       boolean,
    tag_ids             integer[]
);

alter table pixiv_illust
    owner to postgres;

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
    illust_type           varchar(10),
    caption_html          text,
    title                 varchar(256),
    date                  timestamp,
    media_ids             integer[],
    ugoira_frame_duration integer[]
);

alter table pixiv_illust_history
    owner to postgres;

create table pixiv_novel
(
    id                  serial
        constraint pixiv_novel_pk
            primary key,
    parent_id           integer
        constraint pixiv_novel_pixiv_user_null_fk
            references pixiv_user,
    source_id           varchar(24),
    source_inaccessible boolean,
    last_modified       timestamp,
    total_bookmarks     integer,
    total_view          integer,
    is_bookmarked       boolean,
    tag_ids             integer[]
);

alter table pixiv_novel
    owner to postgres;

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
    title          varchar(256),
    caption_html   text,
    text           text,
    date           timestamp
);

alter table pixiv_novel_history
    owner to postgres;

