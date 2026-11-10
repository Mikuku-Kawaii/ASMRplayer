begin;
create table public.user_sync (
    user_id uuid not null references auth.users(id) on delete cascade,
    device_id uuid not null,
    sync_data jsonb not null check (jsonb_typeof(sync_data) = 'object'),
    updated_at timestamptz not null default now(),
    primary key (user_id, device_id)
);
alter table public.user_sync enable row level security;
revoke all on public.user_sync from public, anon, authenticated;
grant select, insert (user_id, device_id, sync_data), update (user_id, device_id, sync_data)
    on public.user_sync to authenticated;
create policy read_own on public.user_sync for select to authenticated
    using (user_id = (select auth.uid()));
create policy insert_own on public.user_sync for insert to authenticated
    with check (user_id = (select auth.uid()));
create policy update_own on public.user_sync for update to authenticated
    using (user_id = (select auth.uid())) with check (user_id = (select auth.uid()));
create function public.touch_user_sync() returns trigger
language plpgsql set search_path = '' as $$
begin
    new.updated_at := statement_timestamp();
    return new;
end;
$$;
create trigger user_sync_updated before update on public.user_sync
    for each row execute function public.touch_user_sync();
commit;
