create table pixiv_illust
(
    id                  bigint auto_increment
        primary key,
    parent_id           bigint      null,
    source_id           varchar(32) null,
    source_inaccessible tinyint(1)  not null,
    last_modified       datetime    null,
    total_bookmarks     int         null,
    total_view          int         null,
    is_bookmarked       int         null,
    constraint pixiv_illust_source_id_uindex
        unique (source_id)
);

create index pixiv_illust_parent_id_index
    on pixiv_illust (parent_id);

create table pixiv_illust_history
(
    id            bigint auto_increment
        primary key,
    item_id       bigint       not null,
    last_modified datetime     null,
    illust_type   varchar(10)  null,
    caption_html  text         null,
    title         varchar(255) null,
    date          datetime     null
);

create index pixiv_illust_history_item_id_index
    on pixiv_illust_history (item_id);

create table pixiv_illust_history_media
(
    history_id bigint not null,
    media_id   bigint not null,
    primary key (history_id, media_id)
);

create table pixiv_illust_tag
(
    illust_id bigint not null,
    tag_id    bigint not null,
    primary key (illust_id, tag_id)
);

create table pixiv_image_color
(
    id       bigint auto_increment
        primary key,
    image_id bigint not null,
    h        float  not null,
    s        float  not null,
    v        float  not null
);

create index pixiv_image_color_image_id_index
    on pixiv_image_color (image_id);

create table pixiv_media
(
    id         bigint auto_increment
        primary key,
    url        varchar(255) null,
    size       bigint       null,
    mime       varchar(16)  null,
    local_path varchar(255) null,
    width      int          null,
    height     int          null,
    constraint pixiv_image_url_uindex
        unique (url)
);

create table pixiv_novel
(
    id                  bigint auto_increment
        primary key,
    parent_id           bigint      null,
    source_id           varchar(64) not null,
    source_inaccessible tinyint(1)  null,
    last_modified       datetime    null,
    total_bookmarks     int         null,
    total_view          int         null,
    is_bookmarked       tinyint(1)  null,
    constraint table_name_source_id_uindex
        unique (source_id)
);

create index pixiv_novel_parent_id_index
    on pixiv_novel (parent_id);

create table pixiv_novel_history
(
    id             bigint auto_increment
        primary key,
    item_id        bigint       not null,
    cover_image_id bigint       null,
    last_modified  datetime     null,
    title          varchar(255) null,
    caption_html   text         null,
    text           mediumtext   null,
    date           datetime     null
);

create index pixiv_novel_history_cover_image_id_index
    on pixiv_novel_history (cover_image_id);

create index pixiv_novel_history_item_id_index
    on pixiv_novel_history (item_id);

create table pixiv_novel_history_media
(
    history_id bigint not null,
    media_id   bigint not null,
    primary key (history_id, media_id)
);

create table pixiv_novel_tag
(
    novel_id bigint not null,
    tag_id   bigint not null,
    primary key (novel_id, tag_id)
);

create table pixiv_tag
(
    id        bigint auto_increment
        primary key,
    protected tinyint(1) null
);

create table pixiv_tag_alias
(
    id     bigint auto_increment
        primary key,
    tag_id bigint       not null,
    alias  varchar(255) not null
);

create index pixiv_tag_alias_alias_index
    on pixiv_tag_alias (alias);

create index pixiv_tag_alias_tag_id_index
    on pixiv_tag_alias (tag_id);

create table pixiv_user
(
    id                     bigint auto_increment
        primary key,
    source_id              varchar(24) null,
    source_inaccessible    tinyint(1)  not null,
    last_modified          datetime    null,
    is_followed            tinyint(1)  null,
    total_following        int         null,
    total_illust_series    int         null,
    total_illusts          int         null,
    total_manga            int         null,
    total_novel_series     int         null,
    total_novels           int         null,
    total_public_bookmarks int         null,
    constraint pixiv_user_id_uindex
        unique (id),
    constraint pixiv_user_source_id_uindex
        unique (source_id)
);

create table pixiv_user_history
(
    id                 bigint auto_increment
        primary key,
    item_id            bigint       not null,
    workspace_image_id bigint       null,
    background_id      bigint       null,
    avatar_id          bigint       null,
    last_modified      datetime     null,
    birth              varchar(10)  null,
    region             varchar(64)  null,
    gender             varchar(10)  null,
    comment            text         null,
    twitter_account    varchar(64)  null,
    web_page           varchar(255) null,
    workspace          text         null
);

create index pixiv_user_history_avatar_id_index
    on pixiv_user_history (avatar_id);

create index pixiv_user_history_background_id_index
    on pixiv_user_history (background_id);

create index pixiv_user_history_item_id_index
    on pixiv_user_history (item_id);

create index pixiv_user_history_workspace_image_id_index
    on pixiv_user_history (workspace_image_id);

