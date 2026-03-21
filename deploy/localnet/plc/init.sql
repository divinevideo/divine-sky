create extension if not exists "pgcrypto";

create table if not exists plc_bootstrap_marker (
  id uuid primary key default gen_random_uuid(),
  created_at timestamptz not null default now(),
  note text not null
);

insert into plc_bootstrap_marker (note)
select 'localnet bootstrap'
where not exists (select 1 from plc_bootstrap_marker);
