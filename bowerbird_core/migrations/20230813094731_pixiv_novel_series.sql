create schema pixiv;

create table pixiv.novel_series
(
    id        bigint generated always as identity
        constraint pixiv_novel_series_pk
            primary key,
    source_id text not null
        constraint pixiv_novel_series_series_id_uindex
            unique,
    title     text
);

alter table public.pixiv_novel
    add series_id bigint;

alter table public.pixiv_novel
    add constraint pixiv_novel_novel_series_id_fk
        foreign key (series_id) references pixiv.novel_series;
